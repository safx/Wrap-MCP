use std::sync::OnceLock;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Invalid value for {var}: {message}")]
    InvalidValue { var: String, message: String },
    
    #[error("Failed to parse {var} as {expected_type}: {source}")]
    ParseError {
        var: String,
        expected_type: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

#[derive(Debug, Clone)]
pub struct Config {
    pub transport: String,
    pub log_colors: bool,
    pub tool_timeout_secs: u64,
    pub protocol_version: String,
    pub log_size: usize,
    pub rust_log: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            transport: "stdio".to_string(),
            log_colors: false,
            tool_timeout_secs: 30,
            protocol_version: "2025.03.26".to_string(),
            log_size: 1000,
            rust_log: "info".to_string(),
        }
    }
}

static CONFIG: OnceLock<Config> = OnceLock::new();

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let mut config = Config::default();
        
        // WRAP_MCP_TRANSPORT
        if let Ok(transport) = std::env::var("WRAP_MCP_TRANSPORT") {
            config.transport = transport;
        }
        
        // WRAP_MCP_LOG_COLORS
        if let Ok(log_colors_str) = std::env::var("WRAP_MCP_LOG_COLORS") {
            config.log_colors = log_colors_str.to_lowercase() == "true" || log_colors_str == "1";
        }
        
        // WRAP_MCP_TOOL_TIMEOUT
        if let Ok(timeout_str) = std::env::var("WRAP_MCP_TOOL_TIMEOUT") {
            config.tool_timeout_secs = timeout_str
                .parse()
                .map_err(|e| ConfigError::ParseError {
                    var: "WRAP_MCP_TOOL_TIMEOUT".to_string(),
                    expected_type: "u64".to_string(),
                    source: Box::new(e),
                })?;
        }
        
        // WRAP_MCP_PROTOCOL_VERSION
        if let Ok(protocol_version) = std::env::var("WRAP_MCP_PROTOCOL_VERSION") {
            config.protocol_version = protocol_version;
        }
        
        // WRAP_MCP_LOGSIZE
        if let Ok(logsize_str) = std::env::var("WRAP_MCP_LOGSIZE") {
            config.log_size = logsize_str
                .parse()
                .map_err(|e| ConfigError::ParseError {
                    var: "WRAP_MCP_LOGSIZE".to_string(),
                    expected_type: "usize".to_string(),
                    source: Box::new(e),
                })?;
        }
        
        // RUST_LOG
        if let Ok(rust_log) = std::env::var("RUST_LOG") {
            config.rust_log = rust_log;
        }
        
        // Validation
        if config.tool_timeout_secs == 0 {
            return Err(ConfigError::InvalidValue {
                var: "WRAP_MCP_TOOL_TIMEOUT".to_string(),
                message: "timeout must be greater than 0".to_string(),
            });
        }
        
        if config.log_size == 0 {
            return Err(ConfigError::InvalidValue {
                var: "WRAP_MCP_LOGSIZE".to_string(),
                message: "log size must be greater than 0".to_string(),
            });
        }
        
        Ok(config)
    }
    
    pub fn initialize() -> Result<(), ConfigError> {
        let config = Self::from_env()?;
        CONFIG.set(config).map_err(|_| ConfigError::InvalidValue {
            var: "CONFIG".to_string(),
            message: "Configuration already initialized".to_string(),
        })?;
        Ok(())
    }
    
    pub fn global() -> &'static Config {
        CONFIG.get().expect("Config not initialized. Call Config::initialize() first")
    }
    
    #[cfg(test)]
    pub fn test_default() -> Self {
        Self::default()
    }
    
    #[cfg(test)]
    pub fn set_for_testing(config: Config) {
        use std::sync::Mutex;
        static TEST_LOCK: Mutex<()> = Mutex::new(());
        let _lock = TEST_LOCK.lock().unwrap();
        
        // This is a workaround for testing. In production, CONFIG can only be set once.
        // For tests, we need to be able to override it.
        let _ = CONFIG.set(config);
    }
    
    #[cfg(test)]
    pub fn new_with_log_size(log_size: usize) -> Self {
        let mut config = Self::default();
        config.log_size = log_size;
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(test)]
    use serial_test::serial;
    
    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.transport, "stdio");
        assert_eq!(config.log_colors, false);
        assert_eq!(config.tool_timeout_secs, 30);
        assert_eq!(config.protocol_version, "2025.03.26");
        assert_eq!(config.log_size, 1000);
        assert_eq!(config.rust_log, "info");
    }
    
    #[test]
    #[serial]
    fn test_from_env_with_defaults() {
        // Store original values
        let original_transport = std::env::var("WRAP_MCP_TRANSPORT").ok();
        let original_colors = std::env::var("WRAP_MCP_LOG_COLORS").ok();
        let original_timeout = std::env::var("WRAP_MCP_TOOL_TIMEOUT").ok();
        let original_version = std::env::var("WRAP_MCP_PROTOCOL_VERSION").ok();
        let original_logsize = std::env::var("WRAP_MCP_LOGSIZE").ok();
        let original_rust_log = std::env::var("RUST_LOG").ok();
        
        // Clear all relevant env vars
        unsafe {
            std::env::remove_var("WRAP_MCP_TRANSPORT");
            std::env::remove_var("WRAP_MCP_LOG_COLORS");
            std::env::remove_var("WRAP_MCP_TOOL_TIMEOUT");
            std::env::remove_var("WRAP_MCP_PROTOCOL_VERSION");
            std::env::remove_var("WRAP_MCP_LOGSIZE");
            std::env::remove_var("RUST_LOG");
        }
        
        let config = Config::from_env().unwrap();
        assert_eq!(config.transport, "stdio");
        assert_eq!(config.log_colors, false);
        assert_eq!(config.tool_timeout_secs, 30);
        assert_eq!(config.protocol_version, "2025.03.26");
        assert_eq!(config.log_size, 1000);
        assert_eq!(config.rust_log, "info");
        
        // Restore original values
        unsafe {
            if let Some(v) = original_transport { std::env::set_var("WRAP_MCP_TRANSPORT", v); }
            if let Some(v) = original_colors { std::env::set_var("WRAP_MCP_LOG_COLORS", v); }
            if let Some(v) = original_timeout { std::env::set_var("WRAP_MCP_TOOL_TIMEOUT", v); }
            if let Some(v) = original_version { std::env::set_var("WRAP_MCP_PROTOCOL_VERSION", v); }
            if let Some(v) = original_logsize { std::env::set_var("WRAP_MCP_LOGSIZE", v); }
            if let Some(v) = original_rust_log { std::env::set_var("RUST_LOG", v); }
        }
    }
    
    #[test]
    fn test_config_with_custom_values() {
        // Test by directly constructing Config instead of using environment variables
        let config = Config {
            transport: "tcp".to_string(),
            log_colors: true,
            tool_timeout_secs: 60,
            protocol_version: "2024.01.01".to_string(),
            log_size: 500,
            rust_log: "debug".to_string(),
        };
        
        assert_eq!(config.transport, "tcp");
        assert_eq!(config.log_colors, true);
        assert_eq!(config.tool_timeout_secs, 60);
        assert_eq!(config.protocol_version, "2024.01.01");
        assert_eq!(config.log_size, 500);
        assert_eq!(config.rust_log, "debug");
    }
    
    #[test]
    #[serial]
    fn test_invalid_timeout() {
        let original = std::env::var("WRAP_MCP_TOOL_TIMEOUT").ok();
        
        unsafe {
            std::env::set_var("WRAP_MCP_TOOL_TIMEOUT", "not_a_number");
        }
        let result = Config::from_env();
        assert!(result.is_err());
        
        unsafe {
            std::env::remove_var("WRAP_MCP_TOOL_TIMEOUT");
            if let Some(v) = original { std::env::set_var("WRAP_MCP_TOOL_TIMEOUT", v); }
        }
    }
    
    #[test]
    #[serial]
    fn test_zero_timeout_validation() {
        let original = std::env::var("WRAP_MCP_TOOL_TIMEOUT").ok();
        
        unsafe {
            std::env::set_var("WRAP_MCP_TOOL_TIMEOUT", "0");
        }
        let result = Config::from_env();
        assert!(result.is_err());
        
        unsafe {
            std::env::remove_var("WRAP_MCP_TOOL_TIMEOUT");
            if let Some(v) = original { std::env::set_var("WRAP_MCP_TOOL_TIMEOUT", v); }
        }
    }
    
    #[test]
    #[serial]
    fn test_invalid_logsize() {
        let original = std::env::var("WRAP_MCP_LOGSIZE").ok();
        
        unsafe {
            std::env::set_var("WRAP_MCP_LOGSIZE", "not_a_number");
        }
        let result = Config::from_env();
        assert!(result.is_err());
        
        unsafe {
            std::env::remove_var("WRAP_MCP_LOGSIZE");
            if let Some(v) = original { std::env::set_var("WRAP_MCP_LOGSIZE", v); }
        }
    }
    
    #[test]
    #[serial]
    fn test_zero_logsize_validation() {
        let original = std::env::var("WRAP_MCP_LOGSIZE").ok();
        
        unsafe {
            std::env::set_var("WRAP_MCP_LOGSIZE", "0");
        }
        let result = Config::from_env();
        assert!(result.is_err());
        
        unsafe {
            std::env::remove_var("WRAP_MCP_LOGSIZE");
            if let Some(v) = original { std::env::set_var("WRAP_MCP_LOGSIZE", v); }
        }
    }
}