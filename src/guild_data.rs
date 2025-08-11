use std::fs;
use std::path::Path;
use anyhow::Result;
use crate::raider_io::{RaiderIOClient, GuildData};

pub fn read_guild_data(file_path: &str) -> Result<Vec<String>> {
    if !Path::new(file_path).exists() {
        return Ok(Vec::new());
    }
    
    let content = fs::read_to_string(file_path)?;
    Ok(content
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect())
}

pub fn read_additional_characters(file_path: &str) -> Result<Vec<(String, String)>> {
    if !Path::new(file_path).exists() {
        return Ok(Vec::new());
    }
    
    let content = fs::read_to_string(file_path)?;
    let mut characters = Vec::new();
    
    for line in content.lines() {
        let parts: Vec<&str> = line.trim().split_whitespace().collect();
        if parts.len() >= 2 {
            let name = parts[0].to_string();
            let realm = parts[1..].join(" ");
            characters.push((realm, name));
        }
    }
    
    Ok(characters)
}

pub async fn fetch_all_guild_data(tier: u8) -> Result<Vec<GuildData>> {
    let client = RaiderIOClient::new();
    let guild_urls = read_guild_data("uaguildlist.txt")?;
    
    if guild_urls.is_empty() {
        return Ok(Vec::new());
    }
    
    // Limit concurrent requests to avoid overwhelming the API
    use futures::stream::{self, StreamExt};
    
    let results = stream::iter(guild_urls.into_iter().enumerate().map(|(i, url)| {
        let client = &client;
        async move {
            // Add small delay to spread out requests
            if i > 0 && i % 5 == 0 {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
            
            match client.fetch_guild_data(&url, tier).await {
                Ok(Some(guild)) => {
                    println!("Successfully fetched data for guild: {}", guild.name);
                    Some(guild)
                }
                Ok(None) => {
                    println!("No data found for guild URL: {}", url);
                    None
                }
                Err(e) => {
                    eprintln!("Failed to fetch guild data for {}: {}", url, e);
                    None
                }
            }
        }
    }))
    .buffer_unordered(5) // Process max 5 requests concurrently
    .collect::<Vec<_>>()
    .await;
    
    let guilds: Vec<GuildData> = results.into_iter().flatten().collect();
    println!("Successfully fetched data for {} guilds", guilds.len());

    Ok(guilds)
}

pub fn sort_guilds(mut guilds: Vec<GuildData>) -> Vec<GuildData> {
    guilds.sort_by(|a, b| {
        let (a_progression, a_difficulty) = parse_progress(&a.progress);
        let (b_progression, b_difficulty) = parse_progress(&b.progress);
        
        let difficulty_order = |d: char| match d {
            'M' => 0,
            'H' => 1,
            'N' => 2,
            _ => 3,
        };
        
        let a_order = difficulty_order(a_difficulty);
        let b_order = difficulty_order(b_difficulty);
        
        match a_order.cmp(&b_order) {
            std::cmp::Ordering::Equal => {
                match b_progression.cmp(&a_progression) {
                    std::cmp::Ordering::Equal => {
                        match (a.rank, b.rank) {
                            (Some(a_rank), Some(b_rank)) => a_rank.cmp(&b_rank),
                            (Some(_), None) => std::cmp::Ordering::Less,
                            (None, Some(_)) => std::cmp::Ordering::Greater,
                            (None, None) => std::cmp::Ordering::Equal,
                        }
                    },
                    other => other,
                }
            },
            other => other,
        }
    });
    
    guilds
}

fn parse_progress(progress: &str) -> (u8, char) {
    let parts: Vec<&str> = progress.split(' ').collect();
    if parts.len() >= 2 {
        let progression = parts[0].split('/').next().unwrap_or("0").parse::<u8>().unwrap_or(0);
        let difficulty = parts[1].chars().next().unwrap_or('N');
        (progression, difficulty)
    } else {
        (0, 'N')
    }
}

pub fn format_guild_list(guilds: &[GuildData], limit: Option<usize>) -> String {
    let limited_guilds = if let Some(limit) = limit {
        &guilds[..guilds.len().min(limit)]
    } else {
        guilds
    };

    limited_guilds
        .iter()
        .enumerate()
        .map(|(i, guild)| {
            let mut info = format!(
                "{}. {}, {}, {}, {} rank",
                i + 1,
                guild.name,
                guild.realm,
                guild.progress,
                guild.rank.map(|r| r.to_string()).unwrap_or_else(|| "N/A".to_string())
            );
            
            if guild.best_percent < 100.0 {
                info.push_str(&format!(", {}% best", guild.best_percent));
            }
            
            if guild.pull_count > 0 {
                info.push_str(&format!(", {} pulls", guild.pull_count));
            }
            
            info
        })
        .collect::<Vec<_>>()
        .join("\n")
}