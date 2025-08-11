/// Centralized logging configuration using tracing
use crate::config::{LogFormat, LoggingConfig};
use crate::error::Result;
use tracing::{info, Level};
// File logging support can be added later
use tracing_subscriber::{
    EnvFilter,
};

/// Initialize the logging system based on configuration
pub fn init_logging(config: &LoggingConfig) -> Result<()> {
    let level = parse_log_level(&config.level)?;
    
    // Create the base filter
    let env_filter = EnvFilter::builder()
        .with_default_directive(level.into())
        .from_env_lossy()
        // Reduce noise from dependencies
        .add_directive("hyper=warn".parse().unwrap())
        .add_directive("reqwest=warn".parse().unwrap())
        .add_directive("serenity=warn".parse().unwrap())
        .add_directive("tokio=warn".parse().unwrap())
        .add_directive("rustls=warn".parse().unwrap());

    // Build the subscriber based on format
    match config.format {
        LogFormat::Json => {
            tracing_subscriber::fmt()
                .json()
                .with_env_filter(env_filter)
                .init();
        },
        LogFormat::Pretty => {
            tracing_subscriber::fmt()
                .pretty()
                .with_env_filter(env_filter)
                .init();
        },
        LogFormat::Compact => {
            tracing_subscriber::fmt()
                .compact()
                .with_env_filter(env_filter)
                .init();
        },
    }

    info!("Logging initialized with level: {}", config.level);

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
    fn test_stdout_layer_creation() {
        // This test just ensures the layers can be created without panicking
        let _ = create_stdout_layer(LogFormat::Json);
        let _ = create_stdout_layer(LogFormat::Pretty);
        let _ = create_stdout_layer(LogFormat::Compact);
    }
}