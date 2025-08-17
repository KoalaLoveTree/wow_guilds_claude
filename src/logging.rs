/// Centralized logging configuration using tracing
use crate::config::{LogFormat, LoggingConfig};
use crate::error::Result;
use tracing::{error, info, warn, Level};
// File logging support can be added later
use tracing_subscriber::{
    EnvFilter,
    fmt,
    layer::SubscriberExt,
    util::SubscriberInitExt,
    Layer,
};
use std::fs;
use std::path::Path;

/// Initialize the logging system based on configuration
pub fn init_logging(config: &LoggingConfig) -> Result<()> {
    let level = parse_log_level(&config.level)?;
    
    // Create the base filter for console (all levels)
    let console_filter = EnvFilter::builder()
        .with_default_directive(level.into())
        .from_env_lossy()
        // Reduce noise from dependencies
        .add_directive("hyper=warn".parse().unwrap())
        .add_directive("reqwest=warn".parse().unwrap())
        .add_directive("serenity=warn".parse().unwrap())
        .add_directive("tokio=warn".parse().unwrap())
        .add_directive("rustls=warn".parse().unwrap());

    if config.file_enabled {
        // Setup file logging for errors and warnings only
        let file_path = config.file_path.as_ref()
            .map(|p| p.as_str())
            .unwrap_or("logs/bot_errors.log");
        
        // Create logs directory if it doesn't exist
        if let Some(parent) = Path::new(file_path).parent() {
            fs::create_dir_all(parent).map_err(|e| {
                crate::error::BotError::Application(format!("Failed to create log directory: {}", e))
            })?;
        }
        
        // Create simple summary log for general application logs
        let summary_appender = tracing_appender::rolling::daily("logs", "summary.log");
        let (summary_writer, summary_guard) = tracing_appender::non_blocking(summary_appender);
        
        // Keep the guard alive by leaking it (required for non-blocking appender)
        std::mem::forget(summary_guard);
        
        // Create file filter for general app logs (info level)
        let summary_filter = EnvFilter::builder()
            .with_default_directive("info".parse().unwrap())
            .from_env_lossy()
            .add_directive("wow_guild_bot=info".parse().unwrap());
        
        // Create summary log layer (detailed errors go to individual files)
        let summary_layer = fmt::layer()
            .json()
            .with_writer(summary_writer)
            .with_filter(summary_filter);
        
        // Initialize with console and summary layers (detailed errors in individual files)
        let subscriber = tracing_subscriber::registry()
            .with(summary_layer);
            
        match config.format {
            LogFormat::Json => {
                subscriber
                    .with(fmt::layer().json().with_filter(console_filter))
                    .init();
            },
            LogFormat::Pretty => {
                subscriber
                    .with(fmt::layer().pretty().with_filter(console_filter))
                    .init();
            },
            LogFormat::Compact => {
                subscriber
                    .with(fmt::layer().compact().with_filter(console_filter))
                    .init();
            },
        }
        
        info!("Logging initialized with level: {} (console + summary: logs/summary.log + individual errors: logs/errors/)", config.level);
    } else {
        // Initialize with console layer only
        match config.format {
            LogFormat::Json => {
                tracing_subscriber::fmt()
                    .json()
                    .with_env_filter(console_filter)
                    .init();
            },
            LogFormat::Pretty => {
                tracing_subscriber::fmt()
                    .pretty()
                    .with_env_filter(console_filter)
                    .init();
            },
            LogFormat::Compact => {
                tracing_subscriber::fmt()
                    .compact()
                    .with_env_filter(console_filter)
                    .init();
            },
        }
        
        info!("Logging initialized with level: {} (console only)", config.level);
    }

    Ok(())
}

/// Parse log level from string
fn parse_log_level(level: &str) -> Result<Level> {
    match level.to_lowercase().as_str() {
        "trace" => Ok(Level::TRACE),
        "debug" => Ok(Level::DEBUG),
        "info" => Ok(Level::INFO),
        "warn" => Ok(Level::WARN),
        "error" => Ok(Level::ERROR),
        _ => Err(crate::error::BotError::invalid_input(format!(
            "Invalid log level: {}. Valid levels are: trace, debug, info, warn, error",
            level
        ))),
    }
}


/// Structured logging macros for common operations
#[macro_export]
macro_rules! log_api_request {
    ($method:expr, $url:expr, $status:expr) => {
        tracing::info!(
            method = $method,
            url = $url,
            status = $status,
            "API request completed"
        );
    };
    ($method:expr, $url:expr, $status:expr, duration = $duration:expr) => {
        tracing::info!(
            method = $method,
            url = $url,
            status = $status,
            duration_ms = $duration,
            "API request completed"
        );
    };
}

#[macro_export]
macro_rules! log_discord_command {
    ($command:expr, $user:expr) => {
        tracing::info!(
            command = $command,
            user = $user,
            "Discord command executed"
        );
    };
    ($command:expr, $user:expr, $guild:expr) => {
        tracing::info!(
            command = $command,
            user = $user,
            guild = $guild,
            "Discord command executed"
        );
    };
}

#[macro_export]
macro_rules! log_rate_limit {
    ($api:expr, $delay:expr) => {
        tracing::warn!(
            api = $api,
            delay_ms = $delay,
            "Rate limit reached, applying delay"
        );
    };
}

#[macro_export]
macro_rules! log_data_processing {
    ($operation:expr, $count:expr) => {
        tracing::info!(
            operation = $operation,
            count = $count,
            "Data processing update"
        );
    };
    ($operation:expr, $count:expr, $total:expr) => {
        tracing::info!(
            operation = $operation,
            processed = $count,
            total = $total,
            progress = format!("{:.1}%", ($count as f32 / $total as f32) * 100.0),
            "Data processing update"
        );
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LoggingConfig;

    #[test]
    fn test_parse_log_level() {
        assert_eq!(parse_log_level("info").unwrap(), Level::INFO);
        assert_eq!(parse_log_level("INFO").unwrap(), Level::INFO);
        assert_eq!(parse_log_level("debug").unwrap(), Level::DEBUG);
        assert_eq!(parse_log_level("error").unwrap(), Level::ERROR);
        assert!(parse_log_level("invalid").is_err());
    }

    #[test]
    fn test_logging_config_creation() {
        // Test default logging config
        let config = LoggingConfig::default();
        assert_eq!(config.level, "info");
        assert_eq!(config.format, LogFormat::Pretty);
        assert!(config.file_enabled);
        assert!(config.file_path.is_some());
    }
}