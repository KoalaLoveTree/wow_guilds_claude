/// Configuration management for the WoW Guild Bot
use crate::error::{BotError, Result};
use config::{Config, ConfigError, Environment, File};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Main application configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    pub discord: DiscordConfig,
    pub raider_io: RaiderIoConfig,
    pub rate_limiting: RateLimitConfig,
    pub data: DataConfig,
    pub database: DatabaseConfig,
    pub logging: LoggingConfig,
}

/// Discord bot configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DiscordConfig {
    pub token: String,
    pub guild_id: Option<u64>,
    pub server_id: Option<String>,
    pub rules_channel_id: Option<String>,
    pub auto_role_id: Option<String>,
    pub auto_role_enabled: bool,
}

/// Raider.io API configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RaiderIoConfig {
    pub api_key: Option<String>,
    pub base_url: String,
    pub timeout_secs: u64,
    pub season: String,
    pub region: Region,
}

/// Rate limiting configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RateLimitConfig {
    pub requests_per_second: u32,
    pub concurrent_requests: usize,
    pub retry_attempts: u32,
    pub retry_delay_secs: u64,
}

/// Data handling configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DataConfig {
    pub backup_enabled: bool,
    pub batch_size: usize,
}

/// Database configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub auto_migrate: bool,
}

/// Logging configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LoggingConfig {
    pub level: String,
    pub format: LogFormat,
    pub file_enabled: bool,
    pub file_path: Option<String>,
}

/// Supported WoW regions
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Region {
    Us,
    Eu,
    Kr,
    Tw,
    Cn,
}

/// Log output formats
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    Json,
    Pretty,
    Compact,
}

impl fmt::Display for Region {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Region::Us => write!(f, "us"),
            Region::Eu => write!(f, "eu"),
            Region::Kr => write!(f, "kr"),
            Region::Tw => write!(f, "tw"),
            Region::Cn => write!(f, "cn"),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            discord: DiscordConfig::default(),
            raider_io: RaiderIoConfig::default(),
            rate_limiting: RateLimitConfig::default(),
            data: DataConfig::default(),
            database: DatabaseConfig::default(),
            logging: LoggingConfig::default(),
        }
    }
}

impl Default for DiscordConfig {
    fn default() -> Self {
        Self {
            token: String::new(),
            guild_id: None,
            server_id: None,
            rules_channel_id: None,
            auto_role_id: None,
            auto_role_enabled: true,
        }
    }
}

impl Default for RaiderIoConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: "https://raider.io/api/v1".to_string(),
            timeout_secs: 15,
            season: "current".to_string(),
            region: Region::Eu,
        }
    }
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_second: 50,    // Increased from 10 to match Python bot speed
            concurrent_requests: 25,    // Increased from 5 to match Python concurrency
            retry_attempts: 3,
            retry_delay_secs: 30,
        }
    }
}

impl Default for DataConfig {
    fn default() -> Self {
        Self {
            backup_enabled: true,
            batch_size: 100,
        }
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: "sqlite://wow_guild_bot.db".to_string(),
            auto_migrate: true,
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: LogFormat::Pretty,
            file_enabled: false,
            file_path: None,
        }
    }
}

impl AppConfig {
    /// Load configuration from multiple sources
    pub fn load() -> Result<Self> {
        let config = Config::builder()
            // Start with default values
            .add_source(config::Config::try_from(&AppConfig::default())?)
            // Add configuration file if it exists
            .add_source(File::with_name("config").required(false))
            // Add environment variables with prefix WGB_
            .add_source(
                Environment::with_prefix("WGB")
                    .prefix_separator("_")
                    .separator("__"),
            )
            // Legacy environment variables support (without prefix)
            .add_source(Self::legacy_env_source())
            .build()?;

        let app_config: AppConfig = config.try_deserialize()?;
        app_config.validate()?;
        Ok(app_config)
    }

    /// Support legacy environment variables for backward compatibility
    fn legacy_env_source() -> Config {
        let mut builder = Config::builder();
        
        // Map legacy environment variables
        if let Ok(token) = std::env::var("DISCORD_TOKEN") {
            builder = builder.set_override("discord.token", token).unwrap();
        }
        if let Ok(server_id) = std::env::var("DISCORD_SERVER_ID") {
            builder = builder.set_override("discord.server_id", server_id).unwrap();
        }
        if let Ok(channel_id) = std::env::var("DISCORD_RULES_CHANNEL_ID") {
            builder = builder.set_override("discord.rules_channel_id", channel_id).unwrap();
        }
        if let Ok(role_id) = std::env::var("DISCORD_AUTO_ROLE_ID") {
            builder = builder.set_override("discord.auto_role_id", role_id).unwrap();
        }
        if let Ok(enabled) = std::env::var("DISCORD_AUTO_ROLE_ENABLED") {
            builder = builder.set_override("discord.auto_role_enabled", enabled.parse::<bool>().unwrap_or(true)).unwrap();
        }
        if let Ok(api_key) = std::env::var("RAIDERIO_API_KEY") {
            builder = builder.set_override("raider_io.api_key", api_key).unwrap();
        }
        if let Ok(season) = std::env::var("SEASON") {
            builder = builder.set_override("raider_io.season", season).unwrap();
        }
        
        builder.build().unwrap_or_else(|_| Config::default())
    }

    /// Validate configuration values
    fn validate(&self) -> Result<()> {
        if self.discord.token.is_empty() {
            return Err(BotError::Config(ConfigError::Message(
                "Discord token is required".to_string(),
            )));
        }

        if self.rate_limiting.requests_per_second == 0 {
            return Err(BotError::Config(ConfigError::Message(
                "Requests per second must be greater than 0".to_string(),
            )));
        }

        if self.rate_limiting.concurrent_requests == 0 {
            return Err(BotError::Config(ConfigError::Message(
                "Concurrent requests must be greater than 0".to_string(),
            )));
        }

        Ok(())
    }

    /// Get request delay in milliseconds based on rate limiting config
    pub fn request_delay_ms(&self) -> u64 {
        1000 / self.rate_limiting.requests_per_second as u64
    }

    /// Check if authenticated API access is available
    pub fn has_api_key(&self) -> bool {
        self.raider_io.api_key.is_some()
    }

    /// Get API key or return empty string
    pub fn api_key(&self) -> &str {
        self.raider_io.api_key.as_deref().unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_region_display() {
        assert_eq!(Region::Us.to_string(), "us");
        assert_eq!(Region::Eu.to_string(), "eu");
    }

    #[test]
    fn test_config_defaults() {
        let config = AppConfig::default();
        assert_eq!(config.raider_io.region, Region::Eu);
        assert_eq!(config.rate_limiting.requests_per_second, 10);
        assert_eq!(config.data.batch_size, 100);
    }

    #[test]
    fn test_request_delay_calculation() {
        let mut config = AppConfig::default();
        config.rate_limiting.requests_per_second = 10;
        assert_eq!(config.request_delay_ms(), 100);
        
        config.rate_limiting.requests_per_second = 5;
        assert_eq!(config.request_delay_ms(), 200);
    }
}