/// Guild data management and fetching operations
use std::fs;
use std::path::Path;
use crate::config::AppConfig;
use crate::database::Database;
use crate::error::Result;
use crate::raider_io::{RaiderIOClient, GuildData};
use crate::types::{GuildUrl, GuildName, PlayerName, RaidTier, RealmName};
use futures::stream::{self, StreamExt};
use tracing::{debug, error, info, warn};

/// Read guild URLs from configuration file
pub fn read_guild_data(file_path: &str) -> Result<Vec<GuildUrl>> {
    if !Path::new(file_path).exists() {
        warn!("Guild list file not found: {}", file_path);
        return Ok(Vec::new());
    }
    
    let content = fs::read_to_string(file_path)?;
    let mut guild_urls = Vec::new();
    
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        
        // Parse guild URL format: "realm=name&guild=guildname" or similar
        if let Some(guild_url) = parse_guild_url(trimmed) {
            guild_urls.push(guild_url);
        } else {
            warn!("Failed to parse guild URL: {}", trimmed);
        }
    }
    
    info!("Loaded {} guild URLs from {}", guild_urls.len(), file_path);
    Ok(guild_urls)
}

/// Parse a guild URL string into a GuildUrl struct
fn parse_guild_url(url_str: &str) -> Option<GuildUrl> {
    // Handle different formats - this is a simplified parser
    // Example: "realm=tarren-mill&name=guild-name"
    let mut realm = None;
    let mut guild = None;
    
    for part in url_str.split('&') {
        if let Some((key, value)) = part.split_once('=') {
            match key {
                "realm" => realm = Some(RealmName::from(value)),
                "name" => guild = Some(GuildName::from(value)),
                _ => {}
            }
        }
    }
    
    match (realm, guild) {
        (Some(realm), Some(guild)) => Some(GuildUrl::new(realm, guild)),
        _ => None,
    }
}

/// Read additional characters from file
pub fn read_additional_characters(file_path: &str) -> Result<Vec<(PlayerName, RealmName)>> {
    if !Path::new(file_path).exists() {
        warn!("Additional characters file not found: {}", file_path);
        return Ok(Vec::new());
    }
    
    let content = fs::read_to_string(file_path)?;
    let mut characters = Vec::new();
    
    for line in content.lines() {
        let parts: Vec<&str> = line.trim().split_whitespace().collect();
        if parts.len() >= 2 {
            let name = PlayerName::from(parts[0]);
            let realm = RealmName::from(parts[1]);
            characters.push((name, realm));
        } else if !line.trim().is_empty() {
            warn!("Invalid character line format: {}", line);
        }
    }
    
    info!("Loaded {} additional characters from {}", characters.len(), file_path);
    Ok(characters)
}

/// Fetch all guild data for a given raid tier (using database)
pub async fn fetch_all_guild_data(tier: RaidTier, config: &AppConfig) -> Result<Vec<GuildData>> {
    let client = RaiderIOClient::from_config(config)?;
    
    // Initialize database and get guild URLs from it
    let database = Database::new(&config.database.url).await?;
    let guild_urls = database.get_all_guilds().await?;
    
    if guild_urls.is_empty() {
        warn!("No guild URLs found");
        return Ok(Vec::new());
    }
    
    info!("Fetching data for {} guilds", guild_urls.len());
    
    // Concurrent guild data fetching (like Python bot - no artificial delays)
    let results = stream::iter(guild_urls.into_iter().map(|url| {
        let client = &client;
        async move {
            debug!("Fetching guild data for: {}", url);
            
            match client.fetch_guild_data(&url, tier).await {
                Ok(Some(guild)) => {
                    info!("Successfully fetched data for guild: {}", guild.name);
                    Some(guild)
                }
                Ok(None) => {
                    debug!("No data found for guild: {}", url);
                    None
                }
                Err(e) => {
                    error!("Failed to fetch guild data for {}: {}", url, e);
                    None
                }
            }
        }
    }))
    .buffer_unordered(config.rate_limiting.concurrent_requests)
    .collect::<Vec<_>>()
    .await;
    
    let guilds: Vec<GuildData> = results.into_iter().flatten().collect();
    info!("Successfully fetched data for {} guilds", guilds.len());

    Ok(guilds)
}

/// Sort guilds by progression and rank
pub fn sort_guilds(mut guilds: Vec<GuildData>) -> Vec<GuildData> {
    guilds.sort_by(|a, b| {
        // First sort by world rank (if available and > 0)
        let rank_a = a.rank.as_ref().filter(|r| r.value() > 0);
        let rank_b = b.rank.as_ref().filter(|r| r.value() > 0);
        
        match (rank_a, rank_b) {
            (Some(rank_a), Some(rank_b)) => rank_a.value().cmp(&rank_b.value()),
            (Some(_), None) => std::cmp::Ordering::Less,  // Ranked guilds come first
            (None, Some(_)) => std::cmp::Ordering::Greater, // Unranked guilds come last
            (None, None) => {
                // If no ranks, sort by best percent descending
                b.best_percent.partial_cmp(&a.best_percent).unwrap_or(std::cmp::Ordering::Equal)
            }
        }
    });
    
    debug!("Sorted {} guilds by progression", guilds.len());
    guilds
}

/// Format guild list for display
pub fn format_guild_list(guilds: &[GuildData], limit: Option<usize>, show_all: bool) -> String {
    if guilds.is_empty() {
        return "No guild data available.".to_string();
    }
    
    let display_count = if show_all {
        guilds.len()
    } else {
        limit.unwrap_or(10).min(guilds.len())
    };
    
    let mut result = String::new();
    result.push_str(&format!("**Guild Rankings (Showing {} of {}):**\n", display_count, guilds.len()));
    
    // Use code block for monospace alignment
    result.push_str("```");
    result.push_str("Rank Guild Name                    Server        Progress  World Rank  Best\n");
    result.push_str("──── ──────────────────────────── ───────────── ───────── ─────────── ────────────\n");
    
    for (i, guild) in guilds.iter().take(display_count).enumerate() {
        let rank_num = format!("#{}", i + 1);
        let guild_name = truncate_and_pad(&guild.name, 28);
        let server = truncate_and_pad(&guild.realm.display_name(), 13);
        let progress = truncate_and_pad(&guild.progress, 9);
        
        let world_rank = match &guild.rank {
            Some(rank) => format!("#{}", rank.value()),
            None => "Unranked".to_string(),
        };
        let world_rank_str = truncate_and_pad(&world_rank, 11);
        
        // Check if progress shows completion or no progress data
        let is_completed = guild.progress.contains("/8 M") && guild.progress.starts_with("8/");
        let has_no_progress = guild.best_percent == 100.0 && guild.pull_count.is_none();
        
        let best_progress = if is_completed || has_no_progress {
            "Complete".to_string()
        } else {
            match guild.pull_count {
                Some(pulls) => format!("{:.1}%({} pulls)", guild.best_percent, pulls),
                None => format!("{:.1}%", guild.best_percent),
            }
        };
        
        result.push_str(&format!(
            "{:<4} {:<28} {:<13} {:<9} {:<11} {}\n",
            rank_num,
            guild_name,
            server,
            progress,
            world_rank_str,
            best_progress
        ));
    }
    
    result.push_str("```");
    result
}

/// Helper function to truncate and pad strings to consistent length for monospace alignment
fn truncate_and_pad(s: &str, target_len: usize) -> String {
    if s.len() >= target_len {
        format!("{}...", &s[..target_len.saturating_sub(3)])
    } else {
        format!("{}{}", s, " ".repeat(target_len - s.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{GuildName, RealmName, WorldRank};

    #[test]
    fn test_table_formatting() {
        let test_guilds = vec![
            GuildData {
                name: GuildName::from("Нехай Щастить"),
                realm: RealmName::from("Tarren Mill"),
                progress: "8/8 M".to_string(),
                rank: Some(WorldRank::new(50)),
                best_percent: 100.0,
                pull_count: None,
            },
            GuildData {
                name: GuildName::from("Very Long Guild Name That Should Be Truncated"),
                realm: RealmName::from("Howling Fjord"),
                progress: "7/8 M".to_string(),
                rank: Some(WorldRank::new(1250)),
                best_percent: 85.5,
                pull_count: Some(120),
            },
            GuildData {
                name: GuildName::from("Short"),
                realm: RealmName::from("Kazzak"),
                progress: "6/8 M".to_string(),
                rank: None,
                best_percent: 75.0,
                pull_count: None,
            },
        ];

        let output = format_guild_list(&test_guilds, Some(10), false);
        println!("Dynamic padding output:\n{}", output);
        
        // Should start with guild rankings header
        assert!(output.starts_with("**Guild Rankings"));
        // Should contain guild names and ranks
        assert!(output.contains("Нехай Щастить"));
        assert!(output.contains("Very Long Guild Name"));
        assert!(output.contains("Short"));
        // Should contain progress and rank info
        assert!(output.contains("8/8 M"));
        assert!(output.contains("7/8 M"));
        assert!(output.contains("#50"));
        assert!(output.contains("#1250"));
    }

    #[test]
    fn test_parse_guild_url() {
        let url = "realm=tarren-mill&name=test-guild";
        let parsed = parse_guild_url(url);
        assert!(parsed.is_some());
        
        let guild_url = parsed.unwrap();
        assert_eq!(guild_url.realm.to_string(), "tarren-mill");
        assert_eq!(guild_url.name.to_string(), "test-guild");
    }

    #[test]
    fn test_parse_invalid_guild_url() {
        let url = "invalid-format";
        let parsed = parse_guild_url(url);
        assert!(parsed.is_none());
    }

    #[test]
    fn test_sort_guilds() {
        let mut guilds = vec![
            GuildData {
                name: GuildName::from("Guild B"),
                realm: RealmName::from("realm1"),
                progress: "5/8 M".to_string(),
                rank: Some(crate::types::WorldRank::from(100)),
                best_percent: 85.0,
                pull_count: Some(50),
            },
            GuildData {
                name: GuildName::from("Guild A"),
                realm: RealmName::from("realm1"),
                progress: "8/8 M".to_string(),
                rank: Some(crate::types::WorldRank::from(50)),
                best_percent: 100.0,
                pull_count: Some(120),
            },
        ];

        let sorted = sort_guilds(guilds);
        assert_eq!(sorted[0].name.to_string(), "Guild A");
        assert_eq!(sorted[1].name.to_string(), "Guild B");
    }
}