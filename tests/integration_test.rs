#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use wrap_mcp::logging::LogStorage;
    use wrap_mcp::proxy::ProxyHandler;

    #[tokio::test]
    async fn test_log_storage() {
        let storage = LogStorage::new();

        // Add some logs
        let req_id = storage
            .add_request(
                "test_tool".to_string(),
                serde_json::json!({"param": "value"}),
            )
            .await;

        storage
            .add_response(
                req_id,
                "test_tool".to_string(),
                serde_json::json!({"result": "success"}),
            )
            .await;

        storage.add_stderr("Test stderr message".to_string()).await;

        // Get logs and verify
        let logs = storage.get_logs(None, None).await;
        assert_eq!(logs.len(), 3);

        // Test filtering
        let filter = wrap_mcp::logging::LogFilter {
            tool_name: Some("test_tool".to_string()),
            entry_type: None,
            after: None,
            before: None,
            keyword: None,
        };

        let filtered_logs = storage.get_logs(None, Some(filter)).await;
        assert_eq!(filtered_logs.len(), 2); // Only request and response

        // Test clear
        storage.clear_logs().await;
        let count = storage.get_log_count().await;
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_log_size_limit() {
        unsafe {
            std::env::set_var("WRAP_MCP_LOGSIZE", "5");
        }
        let storage = LogStorage::new();

        // Add more logs than the limit
        for i in 0..10 {
            storage
                .add_request(format!("tool_{i}"), serde_json::json!({"index": i}))
                .await;
        }

        // Should only keep the last 5
        let count = storage.get_log_count().await;
        assert_eq!(count, 5);

        // Clean up
        unsafe {
            std::env::remove_var("WRAP_MCP_LOGSIZE");
        }
    }

    #[test]
    fn test_proxy_handler_creation() {
        let log_storage = Arc::new(LogStorage::new());
        let proxy = ProxyHandler::new(log_storage.clone());

        // Proxy should be created successfully - proxy holds one reference
        assert_eq!(Arc::strong_count(&proxy.log_storage), 2);
    }
}
