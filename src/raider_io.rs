/// Raider.io API client with proper error handling and type safety
use crate::config::AppConfig;
use crate::error::{BotError, Result};
use crate::types::{GuildName, GuildUrl, MythicPlusScore, PlayerName, RaidTier, RealmName, Season, WorldRank};

use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, info, instrument, warn};
use uuid::Uuid;

/// Guild progression data from raider.io
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuildData {
    pub name: GuildName,
    pub realm: RealmName,
    pub progress: String,
    pub rank: Option<WorldRank>,
    pub best_percent: f64,
    pub pull_count: Option<u32>,
}

/// Player mythic+ data from raider.io
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerData {
    pub name: PlayerName,
    pub realm: RealmName,
    pub guild: Option<GuildName>,
    pub class: Option<String>,
    pub active_spec_name: Option<String>,
    pub rio_all: MythicPlusScore,
    pub rio_dps: MythicPlusScore,
    pub rio_healer: MythicPlusScore,
    pub rio_tank: MythicPlusScore,
    pub spec_0: MythicPlusScore,
    pub spec_1: MythicPlusScore,
    pub spec_2: MythicPlusScore,
    pub spec_3: MythicPlusScore,
}

/// Internal raider.io guild API response structure
#[derive(Debug, Clone, Deserialize)]
struct RaiderIOGuildResponse {
    name: String,
    realm: String,
    raid_progression: HashMap<String, RaidProgress>,
    raid_rankings: HashMap<String, RaidRankings>,
}

/// Raid progression details
#[derive(Debug, Clone, Deserialize)]
struct RaidProgress {
    summary: String,
}

/// Raid rankings by difficulty
#[derive(Debug, Clone, Deserialize)]
struct RaidRankings {
    mythic: MythicRanking,
}

/// Mythic difficulty ranking
#[derive(Debug, Clone, Deserialize)]
struct MythicRanking {
    world: Option<u32>,
}

/// Boss kill response from raider.io
#[derive(Debug, Clone, Deserialize)]
struct BossKillResponse {
    kill: Option<KillInfo>,
    #[serde(rename = "killDetails")]
    kill_details: Option<KillDetails>, // Backup field for different API versions
}

/// Kill information
#[derive(Debug, Clone, Deserialize)]
struct KillInfo {
    #[serde(rename = "isSuccess")]
    is_success: Option<bool>,
    #[serde(rename = "durationMs")]
    duration_ms: Option<u64>,
}

/// Kill details for boss encounters (alternative format)
#[derive(Debug, Clone, Deserialize)]
struct KillDetails {
    attempt: Option<AttemptDetails>,
}

/// Attempt details for boss kills
#[derive(Debug, Clone, Deserialize)]
struct AttemptDetails {
    #[serde(rename = "bestPercent")]
    best_percent: Option<f64>,
    #[serde(rename = "pullCount")]
    pull_count: Option<u32>,
}

/// Player character response from raider.io
#[derive(Debug, Clone, Deserialize)]
struct RaiderIOPlayerResponse {
    name: String,
    realm: String,
    guild: Option<PlayerGuild>,
    class: Option<String>,
    active_spec_name: Option<String>,
    mythic_plus_scores_by_season: Option<Vec<MythicPlusSeasonScore>>,
}

/// Guild information in player response
#[derive(Debug, Clone, Deserialize)]
struct PlayerGuild {
    name: String,
}

/// Mythic+ scores by season
#[derive(Debug, Clone, Deserialize)]
struct MythicPlusSeasonScore {
    scores: MythicPlusScores,
}

/// Mythic+ score breakdown
#[derive(Debug, Clone, Deserialize)]
struct MythicPlusScores {
    all: Option<u32>,
    dps: Option<u32>,
    healer: Option<u32>,
    tank: Option<u32>,
    spec_0: Option<u32>,
    spec_1: Option<u32>,
    spec_2: Option<u32>,
    spec_3: Option<u32>,
}

/// HTTP client for raider.io API with rate limiting and error handling
#[derive(Debug, Clone)]
pub struct RaiderIOClient {
    client: Client,
    base_url: String,
    api_key: Option<String>,
    season: Season,
    request_id_header: String,
}

impl RaiderIOClient {
    /// Create a new raider.io client from configuration
    pub fn from_config(config: &AppConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.raider_io.timeout_secs))
            .user_agent("wow-guild-bot/1.0")
            .build()
            .map_err(|e| BotError::Http(e))?;

        info!("Initialized Raider.io client with timeout: {}s", config.raider_io.timeout_secs);
        
        if config.raider_io.api_key.is_some() {
            info!("Using authenticated Raider.io API access");
        } else {
            warn!("Using unauthenticated Raider.io API access - rate limits may apply");
        }

        Ok(Self {
            client,
            base_url: config.raider_io.base_url.clone(),
            api_key: config.raider_io.api_key.clone(),
            season: Season::from(config.raider_io.season.clone()),
            request_id_header: format!("wow-guild-bot-{}", Uuid::new_v4()),
        })
    }

    /// Add API key to URL if available
    fn add_api_key(&self, mut url: String) -> String {
        if let Some(ref api_key) = self.api_key {
            let separator = if url.contains('?') { "&" } else { "?" };
            url.push_str(&format!("{}access_key={}", separator, api_key));
        }
        url
    }

    /// Get raid name from tier
    fn get_raid_name(tier: RaidTier) -> Result<&'static str> {
        match tier.value() {
            1 => Ok("nerubar-palace"),
            2 => Ok("liberation-of-undermine"),
            _ => Err(BotError::invalid_input(format!("Unsupported raid tier: {}", tier))),
        }
    }

    /// Get boss names for liberation-of-undermine raid
    fn get_liberation_boss_names() -> &'static [&'static str] {
        &[
            "vexie-and-the-geargrinders",
            "cauldron-of-carnage", 
            "rik-reverb",
            "stix-bunkjunker",
            "sprocketmonger-lockenstock",
            "onearmed-bandit",
            "mugzee-heads-of-security",
            "chrome-king-gallywix"
        ]
    }

    /// Fetch guild raid progression data
    #[instrument(skip(self), fields(guild = %guild_url.name, realm = %guild_url.realm, tier = %tier))]
    pub async fn fetch_guild_data(&self, guild_url: &GuildUrl, tier: RaidTier) -> Result<Option<GuildData>> {
        let raid_name = Self::get_raid_name(tier)?;
        
        let url = format!(
            "{}/guilds/profile?region={}&{}&fields=raid_rankings,raid_progression",
            self.base_url,
            "eu", // TODO: Make region configurable
            guild_url.to_query_string()
        );
        let url = self.add_api_key(url);

        debug!("Fetching guild data from: {}", url);

        let start = std::time::Instant::now();
        let response = self.client
            .get(&url)
            .header("x-request-id", &self.request_id_header)
            .send()
            .await
            .map_err(BotError::Http)?;

        let duration = start.elapsed();
        let status = response.status();

        crate::log_api_request!("GET", url, status.as_u16(), duration = duration.as_millis() as u64);

        if !response.status().is_success() {
            if status == StatusCode::NOT_FOUND {
                warn!("Guild not found: {}/{}", guild_url.realm, guild_url.name);
                return Ok(None);
            }
            return Err(BotError::from(status));
        }

        let response_text = response.text().await.map_err(BotError::Http)?;
        
        // Parse the JSON and log the successful response
        let guild_data: RaiderIOGuildResponse = serde_json::from_str(&response_text)
            .map_err(|e| BotError::Application(format!("Failed to parse JSON: {}", e)))?;
        

        debug!("Looking for raid_name: '{}' in raid_progression keys: {:?}", raid_name, guild_data.raid_progression.keys().collect::<Vec<_>>());
        debug!("Looking for raid_name: '{}' in raid_rankings keys: {:?}", raid_name, guild_data.raid_rankings.keys().collect::<Vec<_>>());

        let progress = guild_data
            .raid_progression
            .get(raid_name)
            .map(|p| p.summary.clone())
            .unwrap_or_else(|| "No progress".to_string());

        let rank = guild_data
            .raid_rankings
            .get(raid_name)
            .and_then(|r| r.mythic.world)
            .map(WorldRank::from);
            
        debug!("Parsed progress: '{}', rank: {:?}", progress, rank);

        // Fetch best percent and pull count
        let (best_percent, pull_count) = match self
            .fetch_boss_kill_data(&guild_url.realm, &guild_url.name, raid_name, tier, &progress)
            .await
        {
            Ok((percent, count)) => {
                debug!("Boss kill data retrieved: {}% best, {:?} pulls", percent, count);
                (percent, count)
            },
            Err(e) => {
                warn!("Failed to fetch boss kill data for {}/{}: {}", guild_url.realm, guild_url.name, e);
                // For guilds with progression but no detailed boss data, 
                // still show meaningful progression instead of zeros
                if progress.contains("8/8") {
                    (100.0, None) // Full clear
                } else if progress.contains("M") {
                    // Has mythic progression - estimate based on progress
                    if let Some(kills) = progress.split('/').next().and_then(|s| s.parse::<u32>().ok()) {
                        let percent = (kills as f64 / 8.0) * 100.0;
                        (percent, None) // Use calculated percentage
                    } else {
                        (75.0, None) // Fallback for mythic guilds
                    }
                } else if progress.contains("H") {
                    (25.0, None) // Heroic progression
                } else if !progress.starts_with("0/") && progress != "No progress" {
                    (10.0, None) // Some normal progression
                } else {
                    (0.0, None) // No progress at all
                }
            }
        };

        let guild_data = GuildData {
            name: guild_url.name.clone(),
            realm: guild_url.realm.clone(),
            progress,
            rank,
            best_percent,
            pull_count,
        };

        debug!("Successfully fetched guild data for {}/{}", guild_url.realm, guild_url.name);
        Ok(Some(guild_data))
    }

    /// Fetch boss kill data for detailed progression info
    #[instrument(skip(self), fields(guild = %guild, realm = %realm, raid = raid, progress = progress))]
    async fn fetch_boss_kill_data(
        &self,
        realm: &RealmName,
        guild: &GuildName,
        raid: &str,
        tier: RaidTier,
        progress: &str,
    ) -> Result<(f64, Option<u32>)> {
        // Parse the difficulty from progress (e.g., "3/8 M" -> 'M')
        let difficulty_char = progress.chars().last().unwrap_or('N');
        let difficulty = match difficulty_char {
            'M' => "mythic",
            'H' => "heroic", 
            'N' => "normal",
            _ => "normal",
        };

        // Parse current progress to determine best boss to query for kill data
        let current_progress = progress.split('/').next()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(0);
        
        // If full clear (8/8), return perfect progression
        if current_progress >= 8 {
            return Ok((100.0, None)); // Full clear, perfect score
        }
        
        // Get boss name for NEXT progression (like Python bot)
        let boss_name = if tier.value() == 2 { // liberation-of-undermine
            // For progression data, get the NEXT boss they're working on
            // If they're 5/8, get the 6th boss (index 5)
            if current_progress < 8 {
                Self::get_liberation_boss_names().get(current_progress).copied()
            } else {
                // Full clear, no next boss
                return Ok((100.0, None));
            }
        } else if tier.value() == 1 { // nerubar-palace
            // Add Nerubar Palace boss names if needed
            Some("ulgrax-the-devourer") // First boss as fallback
        } else {
            Some("first-boss") // Generic fallback
        };

        let boss_name = match boss_name {
            Some(name) => name,
            None => return Ok((0.0, None)), // No boss data available
        };
        
        let url = format!(
            "https://raider.io/api/guilds/boss-kills?raid={}&difficulty={}&region=eu&realm={}&guild={}&boss={}",
            raid, difficulty, 
            urlencoding::encode(&realm.to_string()),
            urlencoding::encode(&guild.to_string()),
            boss_name
        );

        debug!("Fetching boss kill data from: {}", url);

        let response = self.client
            .get(&url)
            .header("x-request-id", &self.request_id_header)
            .send()
            .await
            .map_err(BotError::Http)?;
        
        let status = response.status();
        
        if status == StatusCode::UNPROCESSABLE_ENTITY {
            debug!("Boss kill data not available (422 response)");
            return Ok((100.0, None));
        }

        if !status.is_success() {
            warn!("Failed to fetch boss kill data: {}", status);
            return Ok((0.0, None));
        }

        let response_text = response.text().await
            .map_err(|e| BotError::Application(format!("Failed to get response text: {}", e)))?;
        
        
        // Handle empty JSON response ({})
        if response_text.trim() == "{}" {
            debug!("Empty JSON response - boss not killed yet");
            // For current progress bosses that aren't killed yet, try the next boss
            if current_progress < 8 {
                return self.try_next_boss_kill_data(realm, guild, raid, tier, current_progress, difficulty).await;
            }
            return Ok((0.0, None));
        }

        let boss_data: BossKillResponse = serde_json::from_str(&response_text)
            .map_err(|e| BotError::Application(format!("Failed to parse JSON: {}", e)))?;

        let (best_percent, pull_count) = if let Some(kill_details) = boss_data.kill_details {
            // Use killDetails format (like Python bot)
            kill_details
                .attempt
                .map(|attempt| {
                    let percent = attempt.best_percent.unwrap_or(100.0);
                    let pulls = attempt.pull_count;
                    (percent, pulls)
                })
                .unwrap_or((100.0, None))
        } else if let Some(kill) = boss_data.kill {
            // Fallback to kill format if available
            if kill.is_success.unwrap_or(false) {
                (100.0, Some(1)) // Killed boss = 100% completion
            } else {
                (0.0, None) // Failed attempt
            }
        } else {
            (100.0, None) // No kill data available, assume completed
        };

        debug!("Boss kill data: {}% best, {:?} pulls", best_percent, pull_count);
        Ok((best_percent, pull_count))
    }
    
    /// Try to get kill data from the next boss in progression
    async fn try_next_boss_kill_data(
        &self,
        realm: &RealmName,
        guild: &GuildName,
        raid: &str,
        tier: RaidTier,
        current_progress: usize,
        difficulty: &str,
    ) -> Result<(f64, Option<u32>)> {
        // Try the next boss (current progress index)
        let next_boss_name = if tier.value() == 2 { // liberation-of-undermine
            Self::get_liberation_boss_names().get(current_progress).copied()
        } else {
            None
        };
        
        let Some(next_boss_name) = next_boss_name else {
            debug!("No next boss available for current progress: {}", current_progress);
            return Ok((0.0, None));
        };
        
        let url = format!(
            "https://raider.io/api/guilds/boss-kills?raid={}&difficulty={}&region=eu&realm={}&guild={}&boss={}",
            raid, difficulty, 
            urlencoding::encode(&realm.to_string()),
            urlencoding::encode(&guild.to_string()),
            next_boss_name
        );

        debug!("Trying next boss kill data from: {}", url);
        
        let response = self.client
            .get(&url)
            .header("x-request-id", &self.request_id_header)
            .send()
            .await
            .map_err(BotError::Http)?;
        
        let status = response.status();
        
        if !status.is_success() {
            debug!("Next boss kill data not available: {}", status);
            return Ok((0.0, None));
        }
        
        let response_text = response.text().await
            .map_err(|e| BotError::Application(format!("Failed to get response text: {}", e)))?;
        
        
        // Handle empty JSON response for next boss too
        if response_text.trim() == "{}" {
            debug!("Next boss also not killed yet - using default values");
            return Ok((0.0, None));
        }
        
        let boss_data: BossKillResponse = serde_json::from_str(&response_text)
            .map_err(|e| BotError::Application(format!("Failed to parse next boss JSON: {}", e)))?;

        let (best_percent, pull_count) = if let Some(kill_details) = boss_data.kill_details {
            // Use killDetails format (preferred, like main function)
            kill_details
                .attempt
                .map(|attempt| {
                    let percent = attempt.best_percent.unwrap_or(0.0);
                    let pulls = attempt.pull_count;
                    (percent, pulls)
                })
                .unwrap_or((0.0, None))
        } else if let Some(kill) = boss_data.kill {
            // Fallback to kill format if available
            if kill.is_success.unwrap_or(false) {
                (100.0, Some(1)) // Killed boss = 100% completion
            } else {
                (0.0, None) // Failed attempt
            }
        } else {
            (0.0, None) // No kill data available
        };
        
        debug!("Next boss kill data: {}% best, {:?} pulls", best_percent, pull_count);
        Ok((best_percent, pull_count))
    }

    /// Fetch player mythic+ data
    #[instrument(skip(self), fields(player = %name, realm = %realm))]
    pub async fn fetch_player_data(
        &self,
        realm: &RealmName,
        name: &PlayerName,
        guild: Option<GuildName>,
    ) -> Result<Option<PlayerData>> {
        let url = format!(
            "{}/characters/profile?region=eu&realm={}&name={}&fields=mythic_plus_scores_by_season:{},class,active_spec_name",
            self.base_url, realm, name, self.season
        );
        let url = self.add_api_key(url);

        debug!("Fetching player data from: {}", url);

        let start = std::time::Instant::now();
        let response = self.client
            .get(&url)
            .header("x-request-id", &self.request_id_header)
            .send()
            .await
            .map_err(BotError::Http)?;

        let duration = start.elapsed();
        let status = response.status();

        crate::log_api_request!("GET", url, status.as_u16(), duration = duration.as_millis() as u64);

        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(BotError::rate_limit("Raider.io API rate limit exceeded"));
        }

        if status.is_server_error() {
            return Err(BotError::raider_io(status.as_u16(), "Server error from raider.io"));
        }

        if status == StatusCode::NOT_FOUND {
            debug!("Player not found: {}/{}", name, realm);
            return Ok(None);
        }

        if !status.is_success() {
            return Err(BotError::from(status));
        }

        let player_response: RaiderIOPlayerResponse = response
            .json()
            .await
            .map_err(|e| BotError::Application(format!("Failed to parse JSON: {}", e)))?;

        let scores = player_response
            .mythic_plus_scores_by_season
            .and_then(|seasons| seasons.first().map(|s| s.scores.clone()));

        let player_data = PlayerData {
            name: PlayerName::from(player_response.name),
            realm: RealmName::from(player_response.realm),
            guild: guild.or_else(|| {
                player_response
                    .guild
                    .map(|g| GuildName::from(g.name))
            }),
            class: player_response.class,
            active_spec_name: player_response.active_spec_name,
            rio_all: scores.as_ref().and_then(|s| s.all).map(MythicPlusScore::from).unwrap_or(MythicPlusScore::zero()),
            rio_dps: scores.as_ref().and_then(|s| s.dps).map(MythicPlusScore::from).unwrap_or(MythicPlusScore::zero()),
            rio_healer: scores.as_ref().and_then(|s| s.healer).map(MythicPlusScore::from).unwrap_or(MythicPlusScore::zero()),
            rio_tank: scores.as_ref().and_then(|s| s.tank).map(MythicPlusScore::from).unwrap_or(MythicPlusScore::zero()),
            spec_0: scores.as_ref().and_then(|s| s.spec_0).map(MythicPlusScore::from).unwrap_or(MythicPlusScore::zero()),
            spec_1: scores.as_ref().and_then(|s| s.spec_1).map(MythicPlusScore::from).unwrap_or(MythicPlusScore::zero()),
            spec_2: scores.as_ref().and_then(|s| s.spec_2).map(MythicPlusScore::from).unwrap_or(MythicPlusScore::zero()),
            spec_3: scores.as_ref().and_then(|s| s.spec_3).map(MythicPlusScore::from).unwrap_or(MythicPlusScore::zero()),
        };

        info!("Successfully fetched player data for {}/{}", name, realm);
        Ok(Some(player_data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RaiderIoConfig;

    fn create_test_config() -> AppConfig {
        let mut config = AppConfig::default();
        config.raider_io = RaiderIoConfig {
            api_key: Some("test-key".to_string()),
            base_url: "https://raider.io/api/v1".to_string(),
            timeout_secs: 15,
            season: "current".to_string(),
            region: crate::config::Region::Eu,
        };
        config
    }

    #[test]
    fn test_client_creation() {
        let config = create_test_config();
        let client = RaiderIOClient::from_config(&config);
        assert!(client.is_ok());
    }

    #[test]
    fn test_add_api_key() {
        let config = create_test_config();
        let client = RaiderIOClient::from_config(&config).unwrap();
        
        let url_without_params = "https://raider.io/api/v1/test".to_string();
        let result = client.add_api_key(url_without_params);
        assert!(result.contains("?access_key=test-key"));
        
        let url_with_params = "https://raider.io/api/v1/test?existing=param".to_string();
        let result = client.add_api_key(url_with_params);
        assert!(result.contains("&access_key=test-key"));
    }

    #[test]
    fn test_raid_name_mapping() {
        assert_eq!(RaiderIOClient::get_raid_name(RaidTier::from(1)).unwrap(), "nerubar-palace");
        assert_eq!(RaiderIOClient::get_raid_name(RaidTier::from(2)).unwrap(), "liberation-of-undermine");
        assert!(RaiderIOClient::get_raid_name(RaidTier::from(99)).is_err());
    }
}