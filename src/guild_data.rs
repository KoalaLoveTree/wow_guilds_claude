/// Guild data management and fetching operations
use std::fs;
use std::path::Path;
use crate::config::AppConfig;
use crate::database::Database;
use crate::error::Result;
use crate::raider_io::{RaiderIOClient, GuildData};
use crate::types::{GuildUrl, GuildName, PlayerName, RaidTier, RealmName};
use futures::stream::{self, StreamExt};
use std::sync::Arc;
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
    
    let total_guilds = guild_urls.len();
    info!("Fetching data for {} guilds", total_guilds);
    crate::log_data_processing!("starting guild data fetch", 0, total_guilds);
    
    // Track progress
    let progress_counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    
    // Concurrent guild data fetching (like Python bot - no artificial delays)
    let results = stream::iter(guild_urls.into_iter().map(|url| {
        let client = &client;
        let progress_counter = Arc::clone(&progress_counter);
        async move {
            debug!("Fetching guild data for: {}", url);
            
            let result = match client.fetch_guild_data(&url, tier).await {
                Ok(Some(guild)) => {
                    let current = progress_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                    info!(
                        guild = %guild.name,
                        realm = %guild.realm,
                        progress = current,
                        total = total_guilds,
                        "Successfully fetched guild data"
                    );
                    if current % 10 == 0 || current == total_guilds {
                        crate::log_data_processing!("fetching guild data", current, total_guilds);
                    }
                    Some(guild)
                }
                Ok(None) => {
                    let current = progress_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                    debug!(
                        guild_url = %url,
                        progress = current,
                        total = total_guilds,
                        "No data found for guild"
                    );
                    None
                }
                Err(e) => {
                    let current = progress_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                    error!(
                        guild_url = %url,
                        progress = current,
                        total = total_guilds,
                        error = %e,
                        "Failed to fetch guild data"
                    );
                    None
                }
            };
            
            result
        }
    }))
    .buffer_unordered(config.rate_limiting.concurrent_requests)
    .collect::<Vec<_>>()
    .await;
    
    let guilds: Vec<GuildData> = results.into_iter().flatten().collect();
    let successful_count = guilds.len();
    let failed_count = total_guilds - successful_count;
    
    crate::log_data_processing!("guild data fetch complete", total_guilds, total_guilds);
    info!(
        successful = successful_count,
        failed = failed_count,
        total = total_guilds,
        "Guild data fetching completed"
    );
    info!("Successfully fetched data for {} guilds", guilds.len());

    Ok(guilds)
}

/// Difficulty levels in order of importance (higher = better)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Difficulty {
    Lfr = 1,
    Normal = 2,
    Heroic = 3,
    Mythic = 4,
}

impl Difficulty {
    fn from_progress(progress: &str) -> Self {
        let difficulty_char = progress.chars().last().unwrap_or('N');
        match difficulty_char {
            'M' => Difficulty::Mythic,
            'H' => Difficulty::Heroic,
            'N' => Difficulty::Normal,
            _ => {
                // Check for LFR
                if progress.contains("LFR") {
                    Difficulty::Lfr
                } else {
                    Difficulty::Normal
                }
            }
        }
    }
}

/// Parse progression string to extract boss count and difficulty
fn parse_progression(progress: &str) -> (u8, Difficulty) {
    // Parse "X/8 M" format
    let boss_count = progress.split('/')
        .next()
        .and_then(|s| s.trim().parse::<u8>().ok())
        .unwrap_or(0);
    
    let difficulty = Difficulty::from_progress(progress);
    (boss_count, difficulty)
}

/// Compare two progressions considering difficulty hierarchy
fn compare_progression(progress_a: &str, progress_b: &str) -> std::cmp::Ordering {
    let (bosses_a, diff_a) = parse_progression(progress_a);
    let (bosses_b, diff_b) = parse_progression(progress_b);
    
    // First compare difficulty (Mythic > Heroic > Normal > LFR)
    match diff_a.cmp(&diff_b) {
        std::cmp::Ordering::Equal => {
            // Same difficulty, compare boss count
            bosses_a.cmp(&bosses_b)
        }
        other => other
    }
}

/// Sort guilds by progression and rank
pub fn sort_guilds(mut guilds: Vec<GuildData>) -> Vec<GuildData> {
    guilds.sort_by(|a, b| {
        // Parse progression to get difficulty for both guilds
        let (bosses_a, diff_a) = parse_progression(&a.progress);
        let (bosses_b, diff_b) = parse_progression(&b.progress);
        
        // STEP 1: Compare by difficulty first (Mythic > Heroic > Normal > LFR)
        // Higher difficulty should rank higher
        if diff_a != diff_b {
            // Different difficulties - higher difficulty wins
            return diff_b.cmp(&diff_a);
        }
        
        // STEP 2: Same difficulty - compare within difficulty
        
        // Special case: Both are 8/8 Mythic (Cutting Edge) - use world rank
        if diff_a == Difficulty::Mythic && bosses_a == 8 && bosses_b == 8 {
            let rank_a = a.rank.as_ref().filter(|r| r.value() > 0);
            let rank_b = b.rank.as_ref().filter(|r| r.value() > 0);
            
            match (rank_a, rank_b) {
                (Some(rank_a), Some(rank_b)) => rank_a.value().cmp(&rank_b.value()),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a.best_percent.partial_cmp(&b.best_percent).unwrap_or(std::cmp::Ordering::Equal)
            }
        } else {
            // All other cases: ignore world rank, sort by boss count then percent
            match bosses_b.cmp(&bosses_a) {
                std::cmp::Ordering::Equal => {
                    // Same boss count - sort by kill time first, then best percent
                    match (&a.defeated_at, &b.defeated_at) {
                        (Some(time_a), Some(time_b)) => {
                            // Both have kill times - earlier is better
                            time_a.cmp(time_b)
                        }
                        (Some(_), None) => std::cmp::Ordering::Less,    // Guild with kill time beats guild without
                        (None, Some(_)) => std::cmp::Ordering::Greater, // Guild without kill time loses
                        (None, None) => {
                            // Neither has kill time - sort by best percent (lower is better)
                            a.best_percent.partial_cmp(&b.best_percent).unwrap_or(std::cmp::Ordering::Equal)
                        }
                    }
                }
                other => other
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
    result.push_str("Rank Guild Name                              Server               Progress  World Rank  Best\n");
    result.push_str("──── ──────────────────────────────────── ──────────────────── ───────── ─────────── ────────────\n");
    
    for (i, guild) in guilds.iter().take(display_count).enumerate() {
        let rank_num = format!("#{}", i + 1);
        let guild_name = truncate_and_pad(&guild.name, 40);
        let server = truncate_and_pad(&guild.realm.display_name(), 20);
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
            "{:<4} {:<40} {:<20} {:<9} {:<11} {}\n",
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
                defeated_at: None,
            },
            GuildData {
                name: GuildName::from("Very Long Guild Name That Should Be Truncated"),
                realm: RealmName::from("Howling Fjord"),
                progress: "7/8 M".to_string(),
                rank: Some(WorldRank::new(1250)),
                best_percent: 85.5,
                pull_count: Some(120),
                defeated_at: None,
            },
            GuildData {
                name: GuildName::from("Short"),
                realm: RealmName::from("Kazzak"),
                progress: "6/8 M".to_string(),
                rank: None,
                best_percent: 75.0,
                pull_count: None,
                defeated_at: None,
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
                defeated_at: None,
            },
            GuildData {
                name: GuildName::from("Guild A"),
                realm: RealmName::from("realm1"),
                progress: "8/8 M".to_string(),
                rank: Some(crate::types::WorldRank::from(50)),
                best_percent: 100.0,
                pull_count: Some(120),
                defeated_at: None,
            },
        ];

        let sorted = sort_guilds(guilds);
        assert_eq!(sorted[0].name.to_string(), "Guild A");
        assert_eq!(sorted[1].name.to_string(), "Guild B");
    }

    #[test]
    fn test_difficulty_aware_ranking() {
        // Test the specific case: 8/8 N should rank LOWER than 2/8 H
        let mut guilds = vec![
            GuildData {
                name: GuildName::from("Normal Guild"),
                realm: RealmName::from("realm1"),
                progress: "8/8 N".to_string(),  // Full normal clear
                rank: None,  // No world rank
                best_percent: 100.0,
                pull_count: None,
                defeated_at: None,
            },
            GuildData {
                name: GuildName::from("Heroic Guild"),
                realm: RealmName::from("realm1"),
                progress: "2/8 H".to_string(),  // 2 heroic bosses
                rank: None,  // No world rank
                best_percent: 25.0,
                pull_count: None,
                defeated_at: None,
            },
        ];

        let sorted = sort_guilds(guilds);
        // Heroic guild should rank higher than Normal guild
        assert_eq!(sorted[0].name.to_string(), "Heroic Guild");
        assert_eq!(sorted[1].name.to_string(), "Normal Guild");
    }

    #[test]
    fn test_difficulty_hierarchy() {
        // Test full difficulty hierarchy: M > H > N > LFR
        let mut guilds = vec![
            GuildData {
                name: GuildName::from("LFR Guild"),
                realm: RealmName::from("realm1"),
                progress: "8/8 LFR".to_string(),
                rank: None,
                best_percent: 100.0,
                pull_count: None,
                defeated_at: None,
            },
            GuildData {
                name: GuildName::from("Normal Guild"),
                realm: RealmName::from("realm1"),
                progress: "1/8 N".to_string(),
                rank: None,
                best_percent: 12.5,
                pull_count: None,
                defeated_at: None,
            },
            GuildData {
                name: GuildName::from("Heroic Guild"),
                realm: RealmName::from("realm1"),
                progress: "1/8 H".to_string(),
                rank: None,
                best_percent: 12.5,
                pull_count: None,
                defeated_at: None,
            },
            GuildData {
                name: GuildName::from("Mythic Guild"),
                realm: RealmName::from("realm1"),
                progress: "1/8 M".to_string(),
                rank: None,
                best_percent: 12.5,
                pull_count: None,
                defeated_at: None,
            },
        ];

        let sorted = sort_guilds(guilds);
        // Should be ordered: Mythic > Heroic > Normal > LFR
        assert_eq!(sorted[0].name.to_string(), "Mythic Guild");
        assert_eq!(sorted[1].name.to_string(), "Heroic Guild");
        assert_eq!(sorted[2].name.to_string(), "Normal Guild");
        assert_eq!(sorted[3].name.to_string(), "LFR Guild");
    }

    #[test]
    fn test_same_difficulty_boss_count() {
        // Test that within same difficulty, more bosses rank higher
        let mut guilds = vec![
            GuildData {
                name: GuildName::from("3 Heroic"),
                realm: RealmName::from("realm1"),
                progress: "3/8 H".to_string(),
                rank: None,
                best_percent: 37.5,
                pull_count: None,
                defeated_at: None,
            },
            GuildData {
                name: GuildName::from("5 Heroic"),
                realm: RealmName::from("realm1"),
                progress: "5/8 H".to_string(),
                rank: None,
                best_percent: 62.5,
                pull_count: None,
                defeated_at: None,
            },
        ];

        let sorted = sort_guilds(guilds);
        // 5/8 H should rank higher than 3/8 H
        assert_eq!(sorted[0].name.to_string(), "5 Heroic");
        assert_eq!(sorted[1].name.to_string(), "3 Heroic");
    }

    #[test]
    fn test_comprehensive_sorting() {
        // Test comprehensive sorting as specified by user:
        // 1. Difficulty priority: Mythic > Heroic > Normal > LFR
        // 2. Boss count within same difficulty
        // 3. Best percent (lower is better)
        // 4. World rank only for 8/8 Mythic
        let mut guilds = vec![
            GuildData {
                name: GuildName::from("8/8 Normal"),
                realm: RealmName::from("realm1"),
                progress: "8/8 N".to_string(),
                rank: None,
                best_percent: 100.0,
                pull_count: None,
                defeated_at: None,
            },
            GuildData {
                name: GuildName::from("2/8 Heroic"),
                realm: RealmName::from("realm1"),
                progress: "2/8 H".to_string(),
                rank: None,
                best_percent: 25.0,
                pull_count: None,
                defeated_at: None,
            },
            GuildData {
                name: GuildName::from("1/8 Mythic"),
                realm: RealmName::from("realm1"),
                progress: "1/8 M".to_string(),
                rank: None,
                best_percent: 12.5,
                pull_count: None,
                defeated_at: None,
            },
            GuildData {
                name: GuildName::from("8/8 Mythic Good Rank"),
                realm: RealmName::from("realm1"),
                progress: "8/8 M".to_string(),
                rank: Some(crate::types::WorldRank::from(100)),
                best_percent: 100.0,
                pull_count: None,
                defeated_at: None,
            },
            GuildData {
                name: GuildName::from("8/8 Mythic Bad Rank"),
                realm: RealmName::from("realm1"),
                progress: "8/8 M".to_string(),
                rank: Some(crate::types::WorldRank::from(500)),
                best_percent: 100.0,
                pull_count: None,
                defeated_at: None,
            },
            GuildData {
                name: GuildName::from("7/8 Heroic Better Percent"),
                realm: RealmName::from("realm1"),
                progress: "7/8 H".to_string(),
                rank: Some(crate::types::WorldRank::from(1)), // World rank should be ignored for non-8/8M
                best_percent: 87.5,
                pull_count: Some(50),
                defeated_at: None,
            },
            GuildData {
                name: GuildName::from("7/8 Heroic Worse Percent"),
                realm: RealmName::from("realm1"),
                progress: "7/8 H".to_string(),
                rank: Some(crate::types::WorldRank::from(1000)), // World rank should be ignored for non-8/8M
                best_percent: 90.0,
                pull_count: Some(100),
                defeated_at: None,
            },
        ];

        let sorted = sort_guilds(guilds);
        
        // Expected order:
        // 1. 8/8 Mythic Good Rank (8/8 M, rank 100)
        // 2. 8/8 Mythic Bad Rank (8/8 M, rank 500)
        // 3. 1/8 Mythic (1/8 M - any mythic beats any heroic)
        // 4. 7/8 Heroic Better Percent (7/8 H, 87.5% - more bosses than 2/8 H)
        // 5. 7/8 Heroic Worse Percent (7/8 H, 90.0% - same bosses, worse percent)
        // 6. 2/8 Heroic (2/8 H - fewer heroic bosses)
        // 7. 8/8 Normal (8/8 N - full normal clear but lower difficulty)
        
        assert_eq!(sorted[0].name.to_string(), "8/8 Mythic Good Rank");
        assert_eq!(sorted[1].name.to_string(), "8/8 Mythic Bad Rank");
        assert_eq!(sorted[2].name.to_string(), "1/8 Mythic");
        assert_eq!(sorted[3].name.to_string(), "7/8 Heroic Better Percent");
        assert_eq!(sorted[4].name.to_string(), "7/8 Heroic Worse Percent");
        assert_eq!(sorted[5].name.to_string(), "2/8 Heroic");
        assert_eq!(sorted[6].name.to_string(), "8/8 Normal");
    }

    #[test]
    fn test_heroic_world_rank_bug() {
        // Test the specific bug: 8/8 H should rank higher than 6/8 H regardless of world rank
        let mut guilds = vec![
            GuildData {
                name: GuildName::from("6/8 Heroic Good Rank"),
                realm: RealmName::from("realm1"),
                progress: "6/8 H".to_string(),
                rank: Some(crate::types::WorldRank::from(100)), // Good world rank
                best_percent: 75.0,
                pull_count: Some(50),
                defeated_at: None,
            },
            GuildData {
                name: GuildName::from("8/8 Heroic Bad Rank"),
                realm: RealmName::from("realm1"),
                progress: "8/8 H".to_string(),
                rank: Some(crate::types::WorldRank::from(5000)), // Bad world rank
                best_percent: 100.0,
                pull_count: None,
                defeated_at: None,
            },
            GuildData {
                name: GuildName::from("8/8 Heroic No Rank"),
                realm: RealmName::from("realm1"),
                progress: "8/8 H".to_string(),
                rank: None, // No world rank
                best_percent: 100.0,
                pull_count: None,
                defeated_at: None,
            },
        ];

        let sorted = sort_guilds(guilds);
        
        // Expected order (world rank should be IGNORED for heroic):
        // 1. 8/8 Heroic Bad Rank (8 bosses beats 6 bosses regardless of rank)
        // 2. 8/8 Heroic No Rank (8 bosses beats 6 bosses regardless of rank)
        // 3. 6/8 Heroic Good Rank (only 6 bosses, should be last despite good rank)
        
        assert_eq!(sorted[0].name.to_string(), "8/8 Heroic Bad Rank");
        assert_eq!(sorted[1].name.to_string(), "8/8 Heroic No Rank");
        assert_eq!(sorted[2].name.to_string(), "6/8 Heroic Good Rank");
    }

    #[test] 
    fn test_debug_world_rank_issue() {
        // Debug the exact scenario you're seeing
        let mut guilds = vec![
            GuildData {
                name: GuildName::from("8/8 H Guild"),
                realm: RealmName::from("realm1"),
                progress: "8/8 H".to_string(),
                rank: None, // No world rank
                best_percent: 100.0,
                pull_count: None,
                defeated_at: None,
            },
            GuildData {
                name: GuildName::from("6/8 H Guild"),
                realm: RealmName::from("realm1"),
                progress: "6/8 H".to_string(),
                rank: Some(crate::types::WorldRank::from(100)), // Has mythic world rank
                best_percent: 75.0,
                pull_count: Some(50),
                defeated_at: None,
            },
        ];

        let sorted = sort_guilds(guilds);
        
        // Debug output
        println!("Sorted order:");
        for (i, guild) in sorted.iter().enumerate() {
            println!("  {}: {} - {} (rank: {:?})", 
                i + 1, 
                guild.name.to_string(), 
                guild.progress,
                guild.rank
            );
        }
        
        // 8/8 H should rank higher than 6/8 H regardless of world rank
        assert_eq!(sorted[0].name.to_string(), "8/8 H Guild");
        assert_eq!(sorted[1].name.to_string(), "6/8 H Guild");
    }

    #[test]
    fn test_world_rank_bug_reproduction() {
        // Try to reproduce the exact bug: 8/8 H ranking lower than 6/8 H due to world rank
        let mut guilds = vec![
            GuildData {
                name: GuildName::from("6/8 H Good Rank"),
                realm: RealmName::from("realm1"),
                progress: "6/8 H".to_string(),
                rank: Some(crate::types::WorldRank::from(50)), // Very good world rank
                best_percent: 75.0,
                pull_count: Some(100),
                defeated_at: None,
            },
            GuildData {
                name: GuildName::from("8/8 H No Rank"),
                realm: RealmName::from("realm1"), 
                progress: "8/8 H".to_string(),
                rank: None, // No world rank
                best_percent: 100.0,
                pull_count: None,
                defeated_at: None,
            },
            GuildData {
                name: GuildName::from("6/8 H Worse Progress"),
                realm: RealmName::from("realm1"),
                progress: "6/8 H".to_string(), 
                rank: Some(crate::types::WorldRank::from(10)), // Even better world rank
                best_percent: 60.0,
                pull_count: Some(50),
                defeated_at: None,
            },
        ];

        let sorted = sort_guilds(guilds);
        
        println!("\nReproduction test results:");
        for (i, guild) in sorted.iter().enumerate() {
            println!("  {}: {} - {} (rank: {:?}, percent: {}%)", 
                i + 1, 
                guild.name.to_string(), 
                guild.progress,
                guild.rank.as_ref().map(|r| r.value()),
                guild.best_percent
            );
        }
        
        // Expected order should be:
        // 1. 8/8 H No Rank (most bosses killed, world rank irrelevant)
        // 2. 6/8 H Worse Progress (better percentage than other 6/8)  
        // 3. 6/8 H Good Rank (same bosses but worse percentage)
        assert_eq!(sorted[0].name.to_string(), "8/8 H No Rank", "8/8 H should rank first regardless of world rank");
        
        // For the 6/8 guilds, they should be sorted by best_percent (lower is better)
        assert_eq!(sorted[1].name.to_string(), "6/8 H Worse Progress", "Better percentage should rank higher among same boss count");
        assert_eq!(sorted[2].name.to_string(), "6/8 H Good Rank", "World rank should be ignored for non-8/8M guilds");
    }

    #[test]
    fn test_cross_difficulty_world_rank_bug() {
        // Test the bug: heroic guilds with good world ranks beating mythic guilds
        let mut guilds = vec![
            GuildData {
                name: GuildName::from("1/8 M No Rank"),
                realm: RealmName::from("realm1"),
                progress: "1/8 M".to_string(),
                rank: None, // No world rank
                best_percent: 12.5,
                pull_count: Some(100),
                defeated_at: None,
            },
            GuildData {
                name: GuildName::from("8/8 H Good Rank"),
                realm: RealmName::from("realm1"),
                progress: "8/8 H".to_string(),
                rank: Some(crate::types::WorldRank::from(50)), // Very good world rank
                best_percent: 100.0,
                pull_count: None,
                defeated_at: None,
            },
        ];

        let sorted = sort_guilds(guilds);
        
        println!("\nCross-difficulty test results:");
        for (i, guild) in sorted.iter().enumerate() {
            println!("  {}: {} - {} (rank: {:?})", 
                i + 1, 
                guild.name.to_string(), 
                guild.progress,
                guild.rank.as_ref().map(|r| r.value())
            );
        }
        
        // ANY mythic progress should beat ANY heroic progress, regardless of world rank
        assert_eq!(sorted[0].name.to_string(), "1/8 M No Rank", "Any mythic progress should beat any heroic progress");
        assert_eq!(sorted[1].name.to_string(), "8/8 H Good Rank", "Heroic should rank lower than mythic regardless of world rank");
    }

    #[test]
    fn test_actual_bug_reproduction() {
        // This test should FAIL if the bug exists - let me reproduce exactly what you're seeing
        let mut guilds = vec![
            GuildData {
                name: GuildName::from("Should Rank FIRST"),
                realm: RealmName::from("realm1"),
                progress: "8/8 H".to_string(), // Full heroic clear
                rank: None, // No world rank
                best_percent: 100.0,
                pull_count: None,
                defeated_at: None,
            },
            GuildData {
                name: GuildName::from("Should Rank SECOND"),
                realm: RealmName::from("realm1"),
                progress: "6/8 H".to_string(), // Partial heroic
                rank: Some(crate::types::WorldRank::from(1)), // Rank #1 world (very good!)
                best_percent: 75.0,
                pull_count: Some(50),
                defeated_at: None,
            },
        ];

        let sorted = sort_guilds(guilds);
        
        println!("\n=== ACTUAL BUG TEST ===");
        for (i, guild) in sorted.iter().enumerate() {
            println!("Position {}: {} - {} (world rank: {:?})", 
                i + 1, 
                guild.name.to_string(), 
                guild.progress,
                guild.rank.as_ref().map(|r| r.value())
            );
        }
        
        // This assertion should pass if the sorting is correct
        // If the bug exists, this will fail because 6/8 H with good rank beats 8/8 H
        if sorted[0].name.to_string() != "Should Rank FIRST" {
            panic!("BUG CONFIRMED: 6/8 H guild with world rank #{} is beating 8/8 H guild!", 
                sorted[0].rank.as_ref().map(|r| r.value()).unwrap_or(0));
        }
        
        assert_eq!(sorted[0].name.to_string(), "Should Rank FIRST");
        assert_eq!(sorted[1].name.to_string(), "Should Rank SECOND");
    }

    #[test]
    fn test_kill_time_sorting() {
        // Test that earlier kill time ranks higher for same progress
        let mut guilds = vec![
            GuildData {
                name: GuildName::from("Later Kill"),
                realm: RealmName::from("realm1"),
                progress: "3/8 M".to_string(),
                rank: None,
                best_percent: 37.5,
                pull_count: Some(100),
                defeated_at: Some("2024-01-02T10:00:00Z".to_string()), // Later kill
            },
            GuildData {
                name: GuildName::from("Earlier Kill"),
                realm: RealmName::from("realm1"),
                progress: "3/8 M".to_string(),
                rank: None,
                best_percent: 37.5,
                pull_count: Some(100),
                defeated_at: Some("2024-01-01T10:00:00Z".to_string()), // Earlier kill
            },
            GuildData {
                name: GuildName::from("No Kill Time"),
                realm: RealmName::from("realm1"),
                progress: "3/8 M".to_string(),
                rank: None,
                best_percent: 30.0, // Better percent but no kill time
                pull_count: Some(50),
                defeated_at: None,
            },
        ];

        let sorted = sort_guilds(guilds);
        
        println!("\nKill time sorting test results:");
        for (i, guild) in sorted.iter().enumerate() {
            println!("  {}: {} - {} (kill time: {:?})", 
                i + 1, 
                guild.name.to_string(), 
                guild.progress,
                guild.defeated_at
            );
        }
        
        // Expected order:
        // 1. Earlier Kill (has kill time from 2024-01-01)
        // 2. Later Kill (has kill time from 2024-01-02)  
        // 3. No Kill Time (no kill time, despite better percent)
        
        assert_eq!(sorted[0].name.to_string(), "Earlier Kill");
        assert_eq!(sorted[1].name.to_string(), "Later Kill");
        assert_eq!(sorted[2].name.to_string(), "No Kill Time");
    }
}