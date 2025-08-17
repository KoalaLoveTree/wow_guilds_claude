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
        let guild_progress = i + 1;
        
        crate::log_data_processing!("fetching guild rosters", guild_progress, guild_urls.len());
        info!(
            "Processing guild {}/{}: {}", 
            guild_progress, 
            guild_urls.len(), 
            url
        );
        
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
                info!(
                    guild = guild_name,
                    members_count = members.len(),
                    progress = guild_progress,
                    total = guild_urls.len(),
                    "Successfully processed guild roster"
                );
            }
        }
        
        // Small delay between guild requests
        if i > 0 && i % 5 == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }
    
    // Additional characters functionality removed - all member data now comes from guild rosters
    
    info!("Collected {} unique players from guild rosters", data_dict.len());
    crate::log_data_processing!("collecting players from rosters", data_dict.len(), data_dict.len());
    
    // Database will be used instead of JSON file
    info!("Storing member data in temporary database table...");
    
    // Fetch RIO data for all players with proper rate limiting and incremental writing
    let players: Vec<_> = data_dict.keys().cloned().collect();
    let total_players = players.len();
    let mut successful_fetches = 0;
    let mut failed_fetches = 0;
    let mut final_players = Vec::new();
    let mut players_written = 0;
    
    info!("Starting RIO data fetch for {} players at 10 requests/second (writing every 100 players)...", total_players);
    crate::log_data_processing!("starting RIO data fetch", 0, total_players);
    
    let mut results = stream::iter(players.into_iter().enumerate().map(|(i, (realm, name))| {
        let client = &client;
        let data_dict = &data_dict;
        async move {
            // Rate limiting: 10 requests per second = 100ms per request
            if i > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            
            // Log concise progress for each player
            println!("[{}/{}] Fetching RIO data for {}-{}", i + 1, total_players, name, realm);
            
            let guild = data_dict.get(&(realm.clone(), name.clone()))
                .and_then(|p| p.guild.clone());
                
            // Retry logic for rate limiting
            let mut attempts = 0;
            let max_attempts = 10;
            
            loop {
                match client.fetch_player_data(&RealmName::from(realm.clone()), &PlayerName::from(name.clone()), guild.clone()).await {
                    Ok(Some(player_data)) => {
                        println!("[{}/{}] ✓ {}-{} (RIO: {:.1})", i + 1, total_players, player_data.name, player_data.realm, player_data.rio_all.value());
                        if (i + 1) % 100 == 0 {
                            crate::log_data_processing!("fetching player RIO data", i + 1, total_players);
                        }
                        return Some((player_data, true, i));
                    }
                    Ok(None) => {
                        println!("[{}/{}] - {}-{} (No RIO data)", i + 1, total_players, name, realm);
                        if (i + 1) % 500 == 0 {
                            crate::log_data_processing!("fetching player RIO data (with missing data)", i + 1, total_players);
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
                                warn!(
                                    player = %name,
                                    realm = %realm,
                                    attempt = attempts,
                                    max_attempts = max_attempts,
                                    progress = i + 1,
                                    total = total_players,
                                    "Rate limited, waiting 10 seconds before retry"
                                );
                                crate::log_rate_limit!("raider.io", 10000);
                                
                                println!("[{}/{}] Rate limited on {}-{}, waiting 10 seconds (attempt {}/{})", i + 1, total_players, name, realm, attempts + 1, max_attempts);
                                for j in 1..=10 {
                                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                                    if j % 2 == 0 {
                                        println!("  [Rate Limited] {}s remaining...", 10 - j);
                                    }
                                }
                                continue;
                            }
                        }
                        
                        // Check if it's a server error (5xx)
                        if error_msg.contains("500") || error_msg.contains("502") || error_msg.contains("503") {
                            if attempts < max_attempts {
                                warn!(
                                    player = %name,
                                    realm = %realm,
                                    attempt = attempts,
                                    max_attempts = max_attempts,
                                    progress = i + 1,
                                    total = total_players,
                                    error = %error_msg,
                                    "Server error, retrying in 10 seconds"
                                );
                                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                                continue;
                            }
                        }
                        
                        println!("[{}/{}] ✗ {}-{} (Failed: {})", i + 1, total_players, name, realm, e);
                        error!(
                            player = %name,
                            realm = %realm,
                            attempts = attempts,
                            progress = i + 1,
                            total = total_players,
                            error = %e,
                            "Failed to fetch RIO data after max attempts"
                        );
                        
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
                // Log database write progress
                let batch_size = final_players.len() - players_written;
                info!(
                    "Writing batch of {} players to database (total processed: {}/{})",
                    batch_size,
                    final_players.len(),
                    total_players
                );
                crate::log_data_processing!("writing to database", final_players.len(), total_players);
                
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
                info!(
                    stored_count = players_written,
                    successful_fetches = successful_fetches,
                    failed_fetches = failed_fetches,
                    "Successfully stored player batch in database"
                );
            }
        }
    }
    
    // Swap temporary table with active members table
    info!("Swapping temporary table with active members table...");
    database.swap_members_tables().await?;
    
    // Get final statistics
    let (guild_count, member_count) = database.get_stats().await?;
    
    crate::log_data_processing!("final data processing complete", final_players.len(), total_players);
    
    info!(
        successful_fetches = successful_fetches,
        failed_fetches = failed_fetches,
        total_processed = final_players.len(),
        guilds_in_db = guild_count,
        members_in_db = member_count,
        "Data fetching completed successfully"
    );
    
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