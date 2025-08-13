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

/// Configuration for logging functionality
#[derive(Debug, Clone)]
pub struct LogConfig {
    pub log_size: usize,
    pub log_colors: bool,
    pub rust_log: String,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            log_size: 1000,
            log_colors: false,
            rust_log: "info".to_string(),
        }
    }
}

/// Configuration for wrappee process management
#[derive(Debug, Clone)]
pub struct WrappeeConfig {
    pub tool_timeout_secs: u64,
    pub protocol_version: String,
}

impl Default for WrappeeConfig {
    fn default() -> Self {
        Self {
            tool_timeout_secs: 30,
            protocol_version: "2025.03.26".to_string(),
        }
    }
}

/// Configuration for transport layer
#[derive(Debug, Clone)]
pub struct TransportConfig {
    pub transport: String,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            transport: "stdio".to_string(),
        }
    }
}

/// Main configuration container
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub log: LogConfig,
    pub wrappee: WrappeeConfig,
    pub transport: TransportConfig,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let mut config = Config::default();

        // WRAP_MCP_TRANSPORT
        if let Ok(transport) = std::env::var("WRAP_MCP_TRANSPORT") {
            config.transport.transport = transport;
        }

        // WRAP_MCP_LOG_COLORS
        if let Ok(log_colors_str) = std::env::var("WRAP_MCP_LOG_COLORS") {
            config.log.log_colors = log_colors_str.to_lowercase() == "true" || log_colors_str == "1";
        }

        // WRAP_MCP_TOOL_TIMEOUT
        if let Ok(timeout_str) = std::env::var("WRAP_MCP_TOOL_TIMEOUT") {
            config.wrappee.tool_timeout_secs =
                timeout_str.parse().map_err(|e| ConfigError::ParseError {
                    var: "WRAP_MCP_TOOL_TIMEOUT".to_string(),
                    expected_type: "u64".to_string(),
                    source: Box::new(e),
                })?;
        }

        // WRAP_MCP_PROTOCOL_VERSION
        if let Ok(protocol_version) = std::env::var("WRAP_MCP_PROTOCOL_VERSION") {
            config.wrappee.protocol_version = protocol_version;
        }

        // WRAP_MCP_LOGSIZE
        if let Ok(logsize_str) = std::env::var("WRAP_MCP_LOGSIZE") {
            config.log.log_size = logsize_str.parse().map_err(|e| ConfigError::ParseError {
                var: "WRAP_MCP_LOGSIZE".to_string(),
                expected_type: "usize".to_string(),
                source: Box::new(e),
            })?;
        }

        // RUST_LOG
        if let Ok(rust_log) = std::env::var("RUST_LOG") {
            config.log.rust_log = rust_log;
        }

        // Validation
        if config.wrappee.tool_timeout_secs == 0 {
            return Err(ConfigError::InvalidValue {
                var: "WRAP_MCP_TOOL_TIMEOUT".to_string(),
                message: "timeout must be greater than 0".to_string(),
            });
        }

        if config.log.log_size == 0 {
            return Err(ConfigError::InvalidValue {
                var: "WRAP_MCP_LOGSIZE".to_string(),
                message: "log size must be greater than 0".to_string(),
            });
        }

        Ok(config)
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
        assert_eq!(config.transport.transport, "stdio");
        assert!(!config.log.log_colors);
        assert_eq!(config.wrappee.tool_timeout_secs, 30);
        assert_eq!(config.wrappee.protocol_version, "2025.03.26");
        assert_eq!(config.log.log_size, 1000);
        assert_eq!(config.log.rust_log, "info");
    }

    #[test]
    #[serial]
    fn test_from_env_with_defaults() {
        // Store original values and clear env vars
        let env_vars = [
            "WRAP_MCP_TRANSPORT",
            "WRAP_MCP_LOG_COLORS",
            "WRAP_MCP_TOOL_TIMEOUT",
            "WRAP_MCP_PROTOCOL_VERSION",
            "WRAP_MCP_LOGSIZE",
            "RUST_LOG",
        ];

        let original_values: Vec<_> = env_vars
            .iter()
            .map(|&var| (var, std::env::var(var).ok()))
            .collect();

        // Clear all relevant env vars
        unsafe {
            for var in &env_vars {
                std::env::remove_var(var);
            }
        }

        let config = Config::from_env().unwrap();
        assert_eq!(config.transport.transport, "stdio");
        assert!(!config.log.log_colors);
        assert_eq!(config.wrappee.tool_timeout_secs, 30);
        assert_eq!(config.wrappee.protocol_version, "2025.03.26");
        assert_eq!(config.log.log_size, 1000);
        assert_eq!(config.log.rust_log, "info");

        // Restore original values
        unsafe {
            for (var, value) in original_values {
                if let Some(v) = value {
                    std::env::set_var(var, v);
                }
            }
        }
    }

    #[test]
    fn test_config_with_custom_values() {
        // Test by directly constructing Config instead of using environment variables
        let config = Config {
            transport: TransportConfig {
                transport: "tcp".to_string(),
            },
            log: LogConfig {
                log_colors: true,
                log_size: 500,
                rust_log: "debug".to_string(),
            },
            wrappee: WrappeeConfig {
                tool_timeout_secs: 60,
                protocol_version: "2024.01.01".to_string(),
            },
        };

        assert_eq!(config.transport.transport, "tcp");
        assert!(config.log.log_colors);
        assert_eq!(config.wrappee.tool_timeout_secs, 60);
        assert_eq!(config.wrappee.protocol_version, "2024.01.01");
        assert_eq!(config.log.log_size, 500);
        assert_eq!(config.log.rust_log, "debug");
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
            if let Some(v) = original {
                std::env::set_var("WRAP_MCP_TOOL_TIMEOUT", v);
            }
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
            if let Some(v) = original {
                std::env::set_var("WRAP_MCP_TOOL_TIMEOUT", v);
            }
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
            if let Some(v) = original {
                std::env::set_var("WRAP_MCP_LOGSIZE", v);
            }
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
            if let Some(v) = original {
                std::env::set_var("WRAP_MCP_LOGSIZE", v);
            }
        }
    }
}