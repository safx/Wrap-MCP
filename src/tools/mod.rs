use anyhow::Result;
use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::Parameters},
    model::*,
    schemars, tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, schemars::JsonSchema)]
pub struct EchoRequest {
    pub message: String,
}

#[derive(Clone)]
pub struct WrapServer {
    tool_router: ToolRouter<WrapServer>,
}

#[tool_router]
impl WrapServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Echo a message and return it in uppercase")]
    fn echo(&self, Parameters(req): Parameters<EchoRequest>) -> Result<CallToolResult, McpError> {
        tracing::debug!("Echo tool called with message: {}", req.message);

        let uppercase_message = req.message.to_uppercase();

        tracing::warn!(
            "Echo transformation: '{}' -> '{}'",
            req.message,
            uppercase_message
        );

        Ok(CallToolResult::success(vec![Content::text(
            uppercase_message,
        )]))
    }
}

#[tool_handler]
impl ServerHandler for WrapServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "Coding Agent Toolkit for Rust".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            instructions: Some(
                "This is a Coding Agent Toolkit for Rust server with echo tool that transforms messages to uppercase."
                    .to_string(),
            ),
        }
    }
}
