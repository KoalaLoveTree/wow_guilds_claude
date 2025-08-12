use std::collections::HashMap;
use std::fs;
use crate::config::AppConfig;
use crate::database::{Database, DbMember};
use crate::error::Result;
use serde_json;
use crate::raider_io::{RaiderIOClient, PlayerData};
use crate::types::{PlayerName, RealmName, GuildName, MythicPlusScore};
use futures::stream::{self, StreamExt};
use tracing::{info, error, warn};

pub async fn generate_members_data() -> Result<()> {
    let config = AppConfig::load()?;
    info!("Starting member data generation with database workflow...");
    
    let client = RaiderIOClient::from_config(&config)?;
    let mut data_dict: HashMap<(String, String), PlayerData> = HashMap::new();
    
    // Initialize database
    let database = Database::new(&config.database.url).await?;
    
    // Clear temporary table for fresh start
    database.clear_temp_members().await?;
    info!("Cleared temporary members table");
    
    // Get guild URLs from database instead of file
    let guild_urls = database.get_all_guilds().await?.into_iter().map(|url| url.to_query_string()).collect::<Vec<_>>();
    info!("Processing {} guilds from database...", guild_urls.len());
    
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
    
    // Additional characters functionality removed - all member data now comes from guild rosters
    
    println!("Fetching RIO data for {} players...", data_dict.len());
    
    // Database will be used instead of JSON file
    info!("Storing member data in temporary database table...");
    
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
    
    // Process results incrementally and store in database every 100 players
    while let Some(result) = results.next().await {
        if let Some((player, success, _index)) = result {
            final_players.push(player);
            if success {
                successful_fetches += 1;
            } else {
                failed_fetches += 1;
            }
            
            // Store in database every 100 players or on the last player
            if final_players.len() % 100 == 0 || final_players.len() == total_players {
                // Convert and store batch in temporary table
                for player in final_players.iter().skip(players_written) {
                    let db_member = DbMember {
                        id: 0, // Will be auto-generated
                        name: player.name.to_string(),
                        realm: player.realm.to_string(),
                        guild_name: player.guild.as_ref().map(|g| g.to_string()),
                        guild_realm: Some(player.realm.to_string()), // Use player's realm as guild realm
                        class: player.class.clone(),
                        spec: player.active_spec_name.clone(),
                        rio_score: Some(player.rio_all.value() as f64), // Legacy field - kept for compatibility
                        ilvl: None, // Could be added later from character data
                        // Complete RIO data matching PlayerData structure
                        rio_all: player.rio_all.value() as f64,
                        rio_dps: player.rio_dps.value() as f64,
                        rio_healer: player.rio_healer.value() as f64,
                        rio_tank: player.rio_tank.value() as f64,
                        spec_0: player.spec_0.value() as f64,
                        spec_1: player.spec_1.value() as f64,
                        spec_2: player.spec_2.value() as f64,
                        spec_3: player.spec_3.value() as f64,
                        updated_at: chrono::Utc::now(),
                    };
                    
                    if let Err(e) = database.insert_temp_member(&db_member).await {
                        error!("Failed to insert member {}-{}: {}", player.name, player.realm, e);
                    }
                }
                
                players_written = final_players.len();
                info!("Stored {} players in database", players_written);
            }
        }
    }
    
    // Swap temporary table with active members table
    info!("Swapping temporary table with active members table...");
    database.swap_members_tables().await?;
    
    // Get final statistics
    let (guild_count, member_count) = database.get_stats().await?;
    
    info!("Completed data fetching:");
    info!("  - Successfully fetched: {} players", successful_fetches);
    info!("  - Failed/No data: {} players", failed_fetches);
    info!("  - Total processed: {} players", final_players.len());
    info!("  - Guilds in database: {}", guild_count);
    info!("  - Members in database: {}", member_count);
    
    // Optional: Export JSON backup for compatibility with existing tools
    if config.data.backup_enabled {
        info!("Creating JSON backup for compatibility...");
        let members = database.get_members_for_ranking(None).await?;
        let json_data = serde_json::to_string_pretty(&members)?;
        
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let backup_filename = format!("members_backup_{}.json", timestamp);
        fs::write(&backup_filename, json_data)?;
        info!("Created JSON backup: {}", backup_filename);
    }
    
    info!("Member data generation complete! Data stored in database with table swap workflow.");
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