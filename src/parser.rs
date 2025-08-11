use std::collections::HashMap;
use std::fs;
use crate::config::AppConfig;
use crate::error::Result;
use serde_json;
use crate::raider_io::{RaiderIOClient, PlayerData};
use crate::guild_data::{read_guild_data, read_additional_characters};
use crate::types::{PlayerName, RealmName, GuildName, MythicPlusScore};
use futures::stream::{self, StreamExt};

pub async fn generate_members_data() -> Result<()> {
    let config = AppConfig::load()?;
    println!("Starting member data generation...");
    
    let client = RaiderIOClient::from_config(&config)?;
    let mut data_dict: HashMap<(String, String), PlayerData> = HashMap::new();
    
    // Read guild URLs
    let guild_urls = read_guild_data(&config.data.guild_list_file)?.into_iter().map(|url| url.to_query_string()).collect::<Vec<_>>();
    println!("Processing {} guilds...", guild_urls.len());
    
    // Process guilds to get member lists
    for (i, url) in guild_urls.iter().enumerate() {
        if let Ok(guild_data) = fetch_guild_members(&client, &url).await {
            if let Some(members) = guild_data.get("members").and_then(|m| m.as_array()) {
                let guild_name = guild_data.get("name").and_then(|n| n.as_str()).unwrap_or("Unknown");
                
                for member in members {
                    if let Some(character) = member.get("character") {
                        let realm = character.get("realm").and_then(|r| r.as_str()).unwrap_or("Unknown").to_string();
                        let name = character.get("name").and_then(|n| n.as_str()).unwrap_or("Unknown").to_string();
                        let class = character.get("class").and_then(|c| c.as_str()).map(|s| s.to_string());
                        let active_spec_name = character.get("active_spec_name").and_then(|a| a.as_str()).map(|s| s.to_string());
                        
                        if !name.is_empty() && name != "Unknown" {
                            let player_key = (realm.clone(), name.clone());
                            data_dict.insert(player_key, PlayerData {
                                name: PlayerName::from(name),
                                realm: RealmName::from(realm),
                                guild: Some(GuildName::from(guild_name.to_string())),
                                class,
                                active_spec_name,
                                rio_all: MythicPlusScore::zero(),
                                rio_dps: MythicPlusScore::zero(),
                                rio_healer: MythicPlusScore::zero(),
                                rio_tank: MythicPlusScore::zero(),
                                spec_0: MythicPlusScore::zero(),
                                spec_1: MythicPlusScore::zero(),
                                spec_2: MythicPlusScore::zero(),
                                spec_3: MythicPlusScore::zero(),
                            });
                        }
                    }
                }
                println!("Processed guild: {} ({} members)", guild_name, members.len());
            }
        }
        
        // Small delay between guild requests
        if i > 0 && i % 5 == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }
    
    // Add additional characters from file
    if let Ok(additional_chars) = read_additional_characters(&config.data.additional_characters_file) {
        let count = additional_chars.len();
        for (name, realm) in &additional_chars {
            let player_key = (realm.to_string(), name.to_string());
            if !data_dict.contains_key(&player_key) {
                data_dict.insert(player_key, PlayerData {
                    name: name.clone(),
                    realm: realm.clone(),
                    guild: None,
                    class: None,
                    active_spec_name: None,
                    rio_all: MythicPlusScore::zero(),
                    rio_dps: MythicPlusScore::zero(),
                    rio_healer: MythicPlusScore::zero(),
                    rio_tank: MythicPlusScore::zero(),
                    spec_0: MythicPlusScore::zero(),
                    spec_1: MythicPlusScore::zero(),
                    spec_2: MythicPlusScore::zero(),
                    spec_3: MythicPlusScore::zero(),
                });
            }
        }
        println!("Added {} additional characters", count);
    }
    
    println!("Fetching RIO data for {} players...", data_dict.len());
    
    // Initialize JSON file with opening bracket
    fs::write("members.json", "[\n")?;
    
    // Fetch RIO data for all players with proper rate limiting and incremental writing
    let players: Vec<_> = data_dict.keys().cloned().collect();
    let total_players = players.len();
    let mut successful_fetches = 0;
    let mut failed_fetches = 0;
    let mut final_players = Vec::new();
    let mut players_written = 0;
    
    println!("Processing {} players at 10 requests/second (writing every 100 players)...", total_players);
    
    let mut results = stream::iter(players.into_iter().enumerate().map(|(i, (realm, name))| {
        let client = &client;
        let data_dict = &data_dict;
        async move {
            // Rate limiting: 10 requests per second = 100ms per request
            if i > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            
            let guild = data_dict.get(&(realm.clone(), name.clone()))
                .and_then(|p| p.guild.clone());
                
            // Retry logic for rate limiting
            let mut attempts = 0;
            let max_attempts = 3;
            
            loop {
                match client.fetch_player_data(&RealmName::from(realm.clone()), &PlayerName::from(name.clone()), guild.clone()).await {
                    Ok(Some(player_data)) => {
                        if (i + 1) % 100 == 0 {
                            println!("Fetched RIO data for: {} ({}/{})", player_data.name, i + 1, total_players);
                        }
                        return Some((player_data, true, i));
                    }
                    Ok(None) => {
                        if (i + 1) % 500 == 0 {
                            println!("No RIO data found for: {} - {} ({}/{})", name, realm, i + 1, total_players);
                        }
                        return Some((PlayerData {
                            name: PlayerName::from(name.clone()),
                            realm: RealmName::from(realm.clone()), 
                            guild: guild.clone(),
                            class: data_dict.get(&(realm.clone(), name.clone())).and_then(|p| p.class.clone()),
                            active_spec_name: data_dict.get(&(realm.clone(), name.clone())).and_then(|p| p.active_spec_name.clone()),
                            rio_all: MythicPlusScore::zero(),
                            rio_dps: MythicPlusScore::zero(),
                            rio_healer: MythicPlusScore::zero(),
                            rio_tank: MythicPlusScore::zero(),
                            spec_0: MythicPlusScore::zero(),
                            spec_1: MythicPlusScore::zero(),
                            spec_2: MythicPlusScore::zero(),
                            spec_3: MythicPlusScore::zero(),
                        }, false, i));
                    }
                    Err(e) => {
                        attempts += 1;
                        let error_msg = e.to_string();
                        
                        // Check if it's a rate limit error
                        if error_msg.contains("429") || error_msg.contains("rate") || error_msg.contains("limit") {
                            if attempts < max_attempts {
                                println!("Rate limited on {}-{}, waiting 30 seconds... (attempt {}/{})", name, realm, attempts, max_attempts);
                                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                                continue;
                            }
                        }
                        
                        // Check if it's a server error (5xx)
                        if error_msg.contains("500") || error_msg.contains("502") || error_msg.contains("503") {
                            if attempts < max_attempts {
                                println!("Server error for {}-{}, retrying in 5 seconds... (attempt {}/{})", name, realm, attempts, max_attempts);
                                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                                continue;
                            }
                        }
                        
                        if (i + 1) % 1000 == 0 || attempts >= max_attempts {
                            eprintln!("Failed to fetch RIO data for {} - {} after {} attempts: {}", name, realm, attempts, e);
                        }
                        
                        return Some((PlayerData {
                            name: PlayerName::from(name.clone()),
                            realm: RealmName::from(realm.clone()),
                            guild: guild.clone(),
                            class: data_dict.get(&(realm.clone(), name.clone())).and_then(|p| p.class.clone()),
                            active_spec_name: data_dict.get(&(realm.clone(), name.clone())).and_then(|p| p.active_spec_name.clone()),
                            rio_all: MythicPlusScore::zero(),
                            rio_dps: MythicPlusScore::zero(),
                            rio_healer: MythicPlusScore::zero(),
                            rio_tank: MythicPlusScore::zero(),
                            spec_0: MythicPlusScore::zero(),
                            spec_1: MythicPlusScore::zero(),
                            spec_2: MythicPlusScore::zero(),
                            spec_3: MythicPlusScore::zero(),
                        }, false, i));
                    }
                }
            }
        }
    }))
    .buffer_unordered(5); // 5 concurrent requests at 100ms intervals for 10 req/sec
    
    // Process results incrementally and write every 100 players
    while let Some(result) = results.next().await {
        if let Some((player, success, _index)) = result {
            final_players.push(player);
            if success {
                successful_fetches += 1;
            } else {
                failed_fetches += 1;
            }
            
            // Write to file every 100 players or on the last player
            if final_players.len() % 100 == 0 || final_players.len() == total_players {
                // Append the current batch to the file
                let mut file_content = std::fs::OpenOptions::new()
                    .append(true)
                    .open("members.json")?;
                
                use std::io::Write;
                for (i, player) in final_players.iter().enumerate().skip(players_written) {
                    let json_line = serde_json::to_string_pretty(player)?;
                    if players_written > 0 || i > 0 {
                        write!(file_content, ",\n")?;
                    }
                    write!(file_content, "{}", json_line)?;
                }
                file_content.flush()?;
                
                players_written = final_players.len();
                println!("Written {} players to members.json", players_written);
            }
        }
    }
    
    // Close the JSON array
    let mut file_content = std::fs::OpenOptions::new()
        .append(true)
        .open("members.json")?;
    use std::io::Write;
    write!(file_content, "\n]")?;
    file_content.flush()?;
    
    println!("Completed data fetching:");
    println!("  - Successfully fetched: {} players", successful_fetches);
    println!("  - Failed/No data: {} players", failed_fetches);
    println!("  - Total processed: {} players", final_players.len());
    
    // Create a timestamped backup
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let timestamped_filename = format!("members_{}.json", timestamp);
    if let Err(e) = fs::copy("members.json", &timestamped_filename) {
        eprintln!("Warning: Could not save timestamped backup {}: {}", timestamped_filename, e);
    } else {
        println!("Also saved backup as: {}", timestamped_filename);
    }
    
    println!("Successfully generated members.json with {} players!", final_players.len());
    Ok(())
}

async fn fetch_guild_members(client: &RaiderIOClient, guild_url: &str) -> Result<serde_json::Value> {
    let url = format!("http://raider.io/api/v1/guilds/profile?region=eu&{}&fields=members", guild_url);
    // Since add_api_key is private, we'll handle the API key ourselves
    // TODO: We should create a public method for this or use a different approach
    
    let http_client = reqwest::Client::new();
    let response = http_client.get(&url).send().await?;
    let guild_data: serde_json::Value = response.json().await.map_err(|e| crate::error::BotError::Application(format!("Failed to parse guild JSON: {}", e)))?;
    
    Ok(guild_data)
}