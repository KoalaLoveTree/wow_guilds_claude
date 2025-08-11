/// Guild data management and fetching operations
use std::fs;
use std::path::Path;
use crate::config::AppConfig;
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

/// Fetch all guild data for a given raid tier
pub async fn fetch_all_guild_data(tier: RaidTier, config: &AppConfig) -> Result<Vec<GuildData>> {
    let client = RaiderIOClient::from_config(config)?;
    let guild_urls = read_guild_data(&config.data.guild_list_file)?;
    
    if guild_urls.is_empty() {
        warn!("No guild URLs found");
        return Ok(Vec::new());
    }
    
    info!("Fetching data for {} guilds", guild_urls.len());
    
    // Limit concurrent requests to avoid overwhelming the API
    let results = stream::iter(guild_urls.into_iter().enumerate().map(|(i, url)| {
        let client = &client;
        async move {
            // Rate limiting
            let delay_ms = config.request_delay_ms();
            if i > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            }
            
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
        // First sort by world rank (if available)
        match (a.rank.as_ref(), b.rank.as_ref()) {
            (Some(rank_a), Some(rank_b)) => rank_a.value().cmp(&rank_b.value()),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
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
    result.push_str(&format!("**Guild Rankings (Showing {} of {}):**\n\n", display_count, guilds.len()));
    
    for (i, guild) in guilds.iter().take(display_count).enumerate() {
        let rank_str = match &guild.rank {
            Some(rank) => format!("#{}", rank.value()),
            None => "Unranked".to_string(),
        };
        
        result.push_str(&format!(
            "{}. **{}** - *{}*\n   Progress: {} | Rank: {} | Best: {:.1}% ({} pulls)\n",
            i + 1,
            guild.name,
            guild.realm,
            guild.progress,
            rank_str,
            guild.best_percent,
            guild.pull_count
        ));
        
        if i < display_count - 1 {
            result.push('\n');
        }
    }
    
    result
}

#[cfg(test)]
mod tests {
    use super::*;

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
                pull_count: 50,
            },
            GuildData {
                name: GuildName::from("Guild A"),
                realm: RealmName::from("realm1"),
                progress: "8/8 M".to_string(),
                rank: Some(crate::types::WorldRank::from(50)),
                best_percent: 100.0,
                pull_count: 120,
            },
        ];

        let sorted = sort_guilds(guilds);
        assert_eq!(sorted[0].name.to_string(), "Guild A");
        assert_eq!(sorted[1].name.to_string(), "Guild B");
    }
}