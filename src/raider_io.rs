use serde::{Deserialize, Serialize};
use anyhow::Result;
use reqwest::Client;
use std::collections::HashMap;
use std::env;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GuildData {
    pub name: String,
    pub realm: String,
    pub progress: String,
    pub rank: Option<u32>,
    pub best_percent: f64,
    pub pull_count: u32,
}

#[derive(Debug, Deserialize)]
struct RaiderIOGuildResponse {
    name: String,
    realm: String,
    raid_progression: HashMap<String, RaidProgress>,
    raid_rankings: HashMap<String, RaidRankings>,
}

#[derive(Debug, Deserialize)]
struct RaidProgress {
    summary: String,
}

#[derive(Debug, Deserialize)]
struct RaidRankings {
    mythic: MythicRanking,
}

#[derive(Debug, Deserialize)]
struct MythicRanking {
    world: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct BossKillResponse {
    #[serde(rename = "killDetails")]
    kill_details: Option<KillDetails>,
}

#[derive(Debug, Deserialize)]
struct KillDetails {
    attempt: Option<AttemptDetails>,
}

#[derive(Debug, Deserialize)]
struct AttemptDetails {
    #[serde(rename = "bestPercent")]
    best_percent: Option<f64>,
    #[serde(rename = "pullCount")]
    pull_count: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PlayerData {
    pub name: String,
    pub realm: String,
    pub guild: Option<String>,
    pub class: Option<String>,
    pub active_spec_name: Option<String>,
    pub rio_all: u32,
    pub rio_dps: u32,
    pub rio_healer: u32,
    pub rio_tank: u32,
    pub spec_0: u32,
    pub spec_1: u32,
    pub spec_2: u32,
    pub spec_3: u32,
}

#[derive(Debug, Deserialize)]
struct RaiderIOPlayerResponse {
    name: String,
    realm: String,
    class: Option<String>,
    active_spec_name: Option<String>,
    mythic_plus_scores_by_season: Option<Vec<MythicPlusScores>>,
}

#[derive(Debug, Deserialize)]
struct MythicPlusScores {
    scores: MythicPlusScoreBreakdown,
}

#[derive(Debug, Deserialize)]
struct MythicPlusScoreBreakdown {
    all: Option<u32>,
    dps: Option<u32>,
    healer: Option<u32>,
    tank: Option<u32>,
    spec_0: Option<u32>,
    spec_1: Option<u32>,
    spec_2: Option<u32>,
    spec_3: Option<u32>,
}

pub struct RaiderIOClient {
    pub client: Client,
    api_key: Option<String>,
    season: String,
}

impl RaiderIOClient {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(15)) // Increase timeout
            .user_agent("wow-guild-bot/1.0")
            .build()
            .expect("Failed to create HTTP client");
        
        let api_key = env::var("RAIDERIO_API_KEY").ok();
        if api_key.is_some() {
            println!("Raider.io API key loaded successfully");
        }
        
        let season = env::var("SEASON").unwrap_or_else(|_| "current".to_string());
        println!("Using season: {}", season);
        
        Self { client, api_key, season }
    }

    pub fn add_api_key(&self, mut url: String) -> String {
        if let Some(ref api_key) = self.api_key {
            if url.contains('?') {
                url.push_str(&format!("&access_key={}", api_key));
            } else {
                url.push_str(&format!("?access_key={}", api_key));
            }
        }
        url
    }

    pub async fn fetch_guild_data(&self, guild_url: &str, tier: u8) -> Result<Option<GuildData>> {
        let raid_name = match tier {
            1 => "nerubar-palace",
            2 => "liberation-of-undermine",
            _ => return Ok(None),
        };

        let url = format!(
            "http://raider.io/api/v1/guilds/profile?region=eu&{}&fields=raid_rankings,raid_progression",
            guild_url
        );
        let url = self.add_api_key(url);

        let response = self.client.get(&url).send().await?;
        let guild_data: RaiderIOGuildResponse = response.json().await?;

        let progress = guild_data
            .raid_progression
            .get(raid_name)
            .map(|p| p.summary.clone())
            .unwrap_or_else(|| "0/0 N".to_string());

        let rank = guild_data
            .raid_rankings
            .get(raid_name)
            .and_then(|r| r.mythic.world);

        let mut best_percent = 100.0;
        let mut pull_count = 0;

        if let Ok(current_progress) = progress.split('/').next().unwrap_or("0").parse::<u8>() {
            if current_progress < 8 {
                let next_boss = self.get_boss_name(current_progress + 1)?;
                let difficulty = progress.chars().last().unwrap_or('N');
                
                if let Ok((realm, guild_name)) = self.parse_guild_url(guild_url) {
                    if let Ok(boss_data) = self.fetch_boss_kill_data(&realm, &guild_name, raid_name, &next_boss, difficulty).await {
                        best_percent = boss_data.0;
                        pull_count = boss_data.1;
                    }
                }
            }
        }

        Ok(Some(GuildData {
            name: guild_data.name,
            realm: guild_data.realm,
            progress,
            rank,
            best_percent,
            pull_count,
        }))
    }

    fn get_boss_name(&self, boss_number: u8) -> Result<String> {
        let boss_names = [
            "vexie-and-the-geargrinders",
            "cauldron-of-carnage", 
            "rik-reverb",
            "stix-bunkjunker",
            "sprocketmonger-lockenstock",
            "onearmed-bandit",
            "mugzee-heads-of-security",
            "chrome-king-gallywix"
        ];

        boss_names
            .get((boss_number - 1) as usize)
            .map(|&s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("Invalid boss number"))
    }

    fn parse_guild_url(&self, guild_url: &str) -> Result<(String, String)> {
        let parts: Vec<&str> = guild_url.split('&').collect();
        if parts.len() < 2 {
            return Err(anyhow::anyhow!("Invalid guild URL format"));
        }

        let realm = parts[0].replace("realm=", "").replace("%20", "-");
        let guild = parts[1].replace("name=", "");

        Ok((realm, guild))
    }

    async fn fetch_boss_kill_data(
        &self,
        realm: &str,
        guild: &str,
        raid: &str,
        boss: &str,
        difficulty: char,
    ) -> Result<(f64, u32)> {
        let difficulty_suffix = match difficulty {
            'M' => "&difficulty=mythic&region=eu&",
            'H' => "&difficulty=heroic&region=eu&",
            'N' => "&difficulty=normal&region=eu&",
            _ => "&difficulty=normal&region=eu&",
        };

        let url = format!(
            "https://raider.io/api/guilds/boss-kills?raid={}{}&realm={}&guild={}&boss={}",
            raid, difficulty_suffix, realm, guild, boss
        );
        let url = self.add_api_key(url);

        let response = self.client.get(&url).send().await?;
        
        if response.status() == 422 {
            return Ok((100.0, 0));
        }

        let boss_data: BossKillResponse = response.json().await?;

        let best_percent = boss_data
            .kill_details
            .as_ref()
            .and_then(|kd| kd.attempt.as_ref())
            .and_then(|a| a.best_percent)
            .unwrap_or(100.0);

        let pull_count = boss_data
            .kill_details
            .as_ref()
            .and_then(|kd| kd.attempt.as_ref())
            .and_then(|a| a.pull_count)
            .unwrap_or(0);

        Ok((best_percent, pull_count))
    }

    pub async fn fetch_player_data(&self, realm: &str, name: &str, guild: Option<String>) -> Result<Option<PlayerData>> {
        let url = format!(
            "http://raider.io/api/v1/characters/profile?region=eu&realm={}&name={}&fields=mythic_plus_scores_by_season:{},class,active_spec_name",
            realm, name, self.season
        );
        let url = self.add_api_key(url);

        let response = self.client.get(&url).send().await?;
        
        // Check for rate limiting or server errors
        let status = response.status();
        if status == 429 {
            return Err(anyhow::anyhow!("Rate limit exceeded (429)"));
        }
        if status == 404 {
            return Ok(None); // Player not found
        }
        if status.is_server_error() {
            return Err(anyhow::anyhow!("Server error: {}", status));
        }
        if !status.is_success() {
            return Err(anyhow::anyhow!("HTTP error: {}", status));
        }

        let player_data: RaiderIOPlayerResponse = response.json().await?;

        let scores = player_data
            .mythic_plus_scores_by_season
            .and_then(|mut seasons| seasons.pop())
            .map(|season| season.scores);

        let (rio_all, rio_dps, rio_healer, rio_tank, spec_0, spec_1, spec_2, spec_3) = 
            if let Some(scores) = scores {
                (
                    scores.all.unwrap_or(0),
                    scores.dps.unwrap_or(0),
                    scores.healer.unwrap_or(0),
                    scores.tank.unwrap_or(0),
                    scores.spec_0.unwrap_or(0),
                    scores.spec_1.unwrap_or(0),
                    scores.spec_2.unwrap_or(0),
                    scores.spec_3.unwrap_or(0),
                )
            } else {
                (0, 0, 0, 0, 0, 0, 0, 0)
            };

        Ok(Some(PlayerData {
            name: player_data.name,
            realm: player_data.realm,
            guild,
            class: player_data.class,
            active_spec_name: player_data.active_spec_name,
            rio_all,
            rio_dps,
            rio_healer,
            rio_tank,
            spec_0,
            spec_1,
            spec_2,
            spec_3,
        }))
    }
}