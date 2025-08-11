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
    pub pull_count: u32,
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
    #[serde(rename = "killDetails")]
    kill_details: Option<KillDetails>,
}

/// Kill details for boss encounters
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

        let guild_data: RaiderIOGuildResponse = response
            .json()
            .await
            .map_err(|e| BotError::Application(format!("Failed to parse JSON: {}", e)))?;

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

        // Fetch best percent and pull count
        let (best_percent, pull_count) = self
            .fetch_boss_kill_data(&guild_url.realm, &guild_url.name, raid_name, 'M')
            .await
            .unwrap_or((0.0, 0));

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
    #[instrument(skip(self), fields(guild = %guild, realm = %realm, raid = raid, difficulty = %difficulty))]
    async fn fetch_boss_kill_data(
        &self,
        realm: &RealmName,
        guild: &GuildName,
        raid: &str,
        difficulty: char,
    ) -> Result<(f64, u32)> {
        let difficulty_suffix = match difficulty {
            'M' => "&difficulty=mythic&region=eu&",
            'H' => "&difficulty=heroic&region=eu&",
            'N' => "&difficulty=normal&region=eu&",
            _ => "&difficulty=normal&region=eu&",
        };

        // Get the last boss for the raid (simplified - using boss=1 for now)
        let boss_id = 1;
        
        let url = format!(
            "https://raider.io/api/guilds/boss-kills?raid={}{}&realm={}&guild={}&boss={}",
            raid, difficulty_suffix, realm, guild, boss_id
        );
        let url = self.add_api_key(url);

        debug!("Fetching boss kill data from: {}", url);

        let response = self.client
            .get(&url)
            .header("x-request-id", &self.request_id_header)
            .send()
            .await
            .map_err(BotError::Http)?;
        
        if response.status() == StatusCode::UNPROCESSABLE_ENTITY {
            debug!("Boss kill data not available (422 response)");
            return Ok((100.0, 0));
        }

        if !response.status().is_success() {
            warn!("Failed to fetch boss kill data: {}", response.status());
            return Ok((0.0, 0));
        }

        let boss_data: BossKillResponse = response
            .json()
            .await
            .map_err(|e| BotError::Application(format!("Failed to parse JSON: {}", e)))?;

        let (best_percent, pull_count) = boss_data
            .kill_details
            .and_then(|kd| kd.attempt)
            .map(|attempt| {
                (
                    attempt.best_percent.unwrap_or(0.0),
                    attempt.pull_count.unwrap_or(0),
                )
            })
            .unwrap_or((0.0, 0));

        debug!("Boss kill data: {}% best, {} pulls", best_percent, pull_count);
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