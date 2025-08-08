# Wrap-MCP

A transparent MCP (Model Context Protocol) proxy server that sits between MCP clients and MCP servers (wrappees), logging all requests/responses while operating as a transparent proxy.

## Overview

Wrap-MCP wraps existing MCP servers and provides the following features:

- 🔄 Transparent proxy: Appears as a regular MCP server to clients
- 📝 Request/response logging
- 🔍 Log search and display tools (`show_log`)
- ⚠️ Captures and logs stderr output from the wrappee process

## System Architecture

```
MCP Client ◄────────► Wrap-MCP ◄────────► Wrappee (MCP Server)
                          │
                      Log Storage
                     (In-Memory)
```

## Installation

```bash
cargo build --release
```

## Usage

### Command Line

```bash
wrap-mcp [wrap-mcp options] -- <wrappee_command> [wrappee arguments]
```

### Environment Variables

- `WRAP_MCP_TRANSPORT`: Transport method (`stdio` or `http`, default: `stdio`)
- `WRAP_MCP_LOGSIZE`: Maximum number of log entries to retain (default: 1000)
- `RUST_LOG`: Log level configuration (e.g., `info`, `debug`, `trace`)

### Examples

```bash
# Wrap and launch another MCP server
WRAP_MCP_LOGSIZE=500 \
RUST_LOG=info \
cargo run -- my-mcp-server --option1 value1

# Launch with HTTP transport
WRAP_MCP_TRANSPORT=http cargo run -- my-mcp-server

# After building, run directly
./target/release/wrap-mcp -- my-mcp-server --port 8080 --config config.json
```

## Available Tools

### Tools Inherited from Wrappee
All tools provided by the wrappee server are automatically available.

### Wrap-MCP Specific Tools

#### `show_log`
Displays recorded logs.

Parameters:
- `limit`: Maximum number of entries to display (default: 20)
- `tool_name`: Filter by tool name
- `entry_type`: Filter by entry type (`request`, `response`, `error`, `stderr`)
- `format`: Output format (`text` or `json`, default: `text`)

#### `clear_log`
Clears all recorded logs.

## Log Management

- Logs are stored in memory
- When the number of logs exceeds `WRAP_MCP_LOGSIZE`, older logs are automatically removed
- Logs are lost when the process terminates

## Development

### Build
```bash
cargo build
```

### Test
```bash
cargo test
```

### Format
```bash
cargo fmt
```

### Lint
```bash
cargo clippy
```

## License

MIT