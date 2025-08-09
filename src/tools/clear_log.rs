use rmcp::{
    ErrorData as McpError,
    model::{CallToolResult, Content},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::logging::LogStorage;

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ClearLogRequest {}

pub async fn clear_log(
    _req: ClearLogRequest,
    log_storage: &LogStorage,
) -> Result<CallToolResult, McpError> {
    tracing::debug!("clear_log called");

    let count = log_storage.get_log_count().await;
    log_storage.clear_logs().await;

    Ok(CallToolResult::success(vec![Content::text(format!(
        "✅ Cleared {count} log entries"
    ))]))
}
