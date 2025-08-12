/// API request logging module for debugging and analysis
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::Path;
use tokio::fs as async_fs;
use tracing::{error, info};

/// API request log entry structure matching Python bot format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiLogEntry {
    pub timestamp: DateTime<Utc>,
    pub request_type: String,
    pub url: String,
    pub response_status: u16,
    pub response_data: Option<Value>,
    pub error: Option<String>,
}

impl ApiLogEntry {
    /// Create a new API log entry for successful response
    pub fn success(request_type: &str, url: &str, status: u16, response_data: Value) -> Self {
        Self {
            timestamp: Utc::now(),
            request_type: request_type.to_string(),
            url: url.to_string(),
            response_status: status,
            response_data: Some(response_data),
            error: None,
        }
    }

    /// Create a new API log entry for error response
    pub fn error(request_type: &str, url: &str, status: u16, error_msg: &str) -> Self {
        Self {
            timestamp: Utc::now(),
            request_type: request_type.to_string(),
            url: url.to_string(),
            response_status: status,
            response_data: None,
            error: Some(error_msg.to_string()),
        }
    }
}

/// API request logger for raider.io calls
pub struct ApiLogger {
    logs_dir: String,
}

impl ApiLogger {
    /// Create a new API logger with specified logs directory
    pub fn new(logs_dir: &str) -> Self {
        // Create logs directory if it doesn't exist
        if let Err(e) = fs::create_dir_all(logs_dir) {
            error!("Failed to create logs directory {}: {}", logs_dir, e);
        }
        
        Self {
            logs_dir: logs_dir.to_string(),
        }
    }

    /// Log an API request asynchronously
    pub async fn log_request(&self, entry: ApiLogEntry) {
        // Generate filename with timestamp and microseconds
        let timestamp_str = entry.timestamp.format("%Y%m%d_%H%M%S_%3f").to_string();
        let filename = format!("{}_{}.json", entry.request_type, timestamp_str);
        let file_path = Path::new(&self.logs_dir).join(filename);

        // Serialize log entry to JSON
        match serde_json::to_string_pretty(&entry) {
            Ok(json_content) => {
                if let Err(e) = async_fs::write(&file_path, json_content).await {
                    error!("Failed to write API log to {:?}: {}", file_path, e);
                } else {
                    info!("Logged API request to {:?}", file_path);
                }
            }
            Err(e) => {
                error!("Failed to serialize API log entry: {}", e);
            }
        }
    }

    /// Log a successful guild profile request
    pub async fn log_guild_profile(&self, url: &str, status: u16, response_data: Value) {
        let entry = ApiLogEntry::success("guild_profile", url, status, response_data);
        self.log_request(entry).await;
    }

    /// Log a successful boss kill request
    pub async fn log_boss_kill(&self, url: &str, status: u16, response_data: Value) {
        let entry = ApiLogEntry::success("boss_kill", url, status, response_data);
        self.log_request(entry).await;
    }

    /// Log a failed boss kill request (422 status)
    pub async fn log_boss_kill_error(&self, url: &str, status: u16, error_msg: &str) {
        let request_type = if status == 422 {
            "boss_kill_422"
        } else {
            "boss_kill_error"
        };
        let entry = ApiLogEntry::error(request_type, url, status, error_msg);
        self.log_request(entry).await;
    }

    /// Log a general API error
    pub async fn log_error(&self, request_type: &str, url: &str, status: u16, error_msg: &str) {
        let entry = ApiLogEntry::error(request_type, url, status, error_msg);
        self.log_request(entry).await;
    }
}

/// Global API logger instance
static mut API_LOGGER: Option<ApiLogger> = None;

/// Initialize the global API logger
pub fn init_api_logger(logs_dir: &str) {
    unsafe {
        API_LOGGER = Some(ApiLogger::new(logs_dir));
    }
    info!("Initialized API logger with logs directory: {}", logs_dir);
}

/// Get the global API logger instance
pub fn get_api_logger() -> Option<&'static ApiLogger> {
    unsafe { API_LOGGER.as_ref() }
}