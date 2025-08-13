use super::wrap_server::WrapServer;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, model::*, service::RequestContext};

impl ServerHandler for WrapServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2025_03_26,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .enable_tool_list_changed()
                .build(),
            server_info: Implementation {
                name: "Wrap-MCP".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            instructions: Some(
                "This is a transparent MCP wrapper that logs all requests/responses while proxying to a wrapped MCP server."
                    .to_string(),
            ),
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        // Store peer for future notifications
        *self.peer.write().await = Some(context.peer.clone());

        let tools = self.tool_manager.get_all_tools().await;
        Ok(ListToolsResult {
            tools,
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        // Store peer for future notifications
        *self.peer.write().await = Some(context.peer.clone());

        let arguments = request
            .arguments
            .map(serde_json::Value::Object)
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

        // Handle restart_wrapped_server
        if request.name == "restart_wrapped_server" {
            self.restart_wrapped_server().await
        } else {
            self.call_tool_dynamic(&request.name, arguments).await
        }
    }
}
