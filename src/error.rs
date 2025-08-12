/// Error types for the WoW Guild Bot application
use thiserror::Error;

/// Main error type for the application
#[derive(Error, Debug)]
pub enum BotError {
    /// HTTP request errors
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),

    /// JSON serialization/deserialization errors
    #[error("JSON processing failed: {0}")]
    Json(#[from] serde_json::Error),

    /// CSV processing errors
    #[error("CSV processing failed: {0}")]
    Csv(#[from] csv::Error),

    /// File I/O errors
    #[error("File operation failed: {0}")]
    Io(#[from] std::io::Error),

    /// Discord API errors
    #[error("Discord API error: {0}")]
    Discord(#[from] serenity::Error),

    /// Configuration errors
    #[error("Configuration error: {0}")]
    Config(#[from] config::ConfigError),

    /// Database errors
    #[error("Database error: {0}")]
    Database(String),

    /// Rate limiting errors
    #[error("Rate limit exceeded: {message}")]
    RateLimit { message: String },

    /// API errors from raider.io
    #[error("Raider.io API error: {status} - {message}")]
    RaiderIo { status: u16, message: String },

    /// Data parsing errors
    #[error("Data parsing failed: {0}")]
    Parse(String),

    /// Guild not found
    #[error("Guild not found: {guild_name} on {realm}")]
    GuildNotFound { guild_name: String, realm: String },

    /// Player not found
    #[error("Player not found: {player_name} on {realm}")]
    PlayerNotFound { player_name: String, realm: String },

    /// Invalid input data
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Generic application error
    #[error("Application error: {0}")]
    Application(String),
}

/// Result type alias for the application
pub type Result<T> = std::result::Result<T, BotError>;

impl BotError {
    /// Create a rate limit error
    pub fn rate_limit<S: Into<String>>(message: S) -> Self {
        Self::RateLimit {
            message: message.into(),
        }
    }

    /// Create a raider.io API error
    pub fn raider_io(status: u16, message: impl Into<String>) -> Self {
        Self::RaiderIo {
            status,
            message: message.into(),
        }
    }

    /// Create a parse error
    pub fn parse<S: Into<String>>(message: S) -> Self {
        Self::Parse(message.into())
    }

    /// Create a guild not found error
    pub fn guild_not_found(guild_name: impl Into<String>, realm: impl Into<String>) -> Self {
        Self::GuildNotFound {
            guild_name: guild_name.into(),
            realm: realm.into(),
        }
    }

    /// Create a player not found error
    pub fn player_not_found(player_name: impl Into<String>, realm: impl Into<String>) -> Self {
        Self::PlayerNotFound {
            player_name: player_name.into(),
            realm: realm.into(),
        }
    }

    /// Create an invalid input error
    pub fn invalid_input<S: Into<String>>(message: S) -> Self {
        Self::InvalidInput(message.into())
    }

    /// Create an application error
    pub fn application<S: Into<String>>(message: S) -> Self {
        Self::Application(message.into())
    }

    /// Check if this is a rate limit error
    pub fn is_rate_limit(&self) -> bool {
        matches!(self, Self::RateLimit { .. })
    }

    /// Check if this is a server error (5xx)
    pub fn is_server_error(&self) -> bool {
        match self {
            Self::RaiderIo { status, .. } => *status >= 500 && *status < 600,
            _ => false,
        }
    }

    /// Check if this is a client error (4xx)
    pub fn is_client_error(&self) -> bool {
        match self {
            Self::RaiderIo { status, .. } => *status >= 400 && *status < 500,
            _ => false,
        }
    }
}

/// Convert HTTP status codes to appropriate errors
impl From<reqwest::StatusCode> for BotError {
    fn from(status: reqwest::StatusCode) -> Self {
        let status_code = status.as_u16();
        let message = match status_code {
            429 => "Rate limit exceeded".to_string(),
            404 => "Resource not found".to_string(),
            500..=599 => "Server error".to_string(),
            _ => format!("HTTP error: {}", status),
        };

        Self::RaiderIo {
            status: status_code,
            message,
        }
    }
}

/// Convert anyhow errors to BotError
impl From<anyhow::Error> for BotError {
    fn from(error: anyhow::Error) -> Self {
        Self::Application(error.to_string())
    }
}