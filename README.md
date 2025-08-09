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

#### Options

- `--ansi`: Preserve ANSI escape sequences in stderr logs
  - By default, Wrap-MCP removes ANSI escape sequences using a hybrid approach:
    - Sets `NO_COLOR=1`, `CLICOLOR=0`, and `RUST_LOG_STYLE=never` environment variables for the wrappee
    - Additionally removes any remaining ANSI escape sequences from stderr output
  - Use this option to preserve the original formatting

- `-w`: Watch the wrapped binary file for changes and automatically restart
  - Monitors the wrappee binary file for modifications
  - Automatically restarts the wrapped server when the binary is updated
  - Uses a 2-second debounce to handle multiple rapid file changes during compilation
  - Shows old and new PIDs in logs for verification
  - Useful for development when frequently recompiling the wrapped server

### Environment Variables

- `WRAP_MCP_TRANSPORT`: Transport method (`stdio` or `http`, default: `stdio`)
- `WRAP_MCP_LOGSIZE`: Maximum number of log entries to retain (default: 1000)
- `RUST_LOG`: Log level configuration (e.g., `info`, `debug`, `trace`)

### Examples

```bash
# Wrap and launch another MCP server (ANSI removed by default)
WRAP_MCP_LOGSIZE=500 \
RUST_LOG=info \
cargo run -- my-mcp-server --option1 value1

# Launch with HTTP transport
WRAP_MCP_TRANSPORT=http cargo run -- my-mcp-server

# Launch while preserving ANSI escape sequences
cargo run -- --ansi -- my-mcp-server --option1 value1

# After building, run directly (ANSI removed by default)
./target/release/wrap-mcp -- my-mcp-server --port 8080 --config config.json

# Preserve ANSI escape sequences
./target/release/wrap-mcp --ansi -- my-mcp-server --port 8080

# Watch binary file for changes and auto-restart
./target/release/wrap-mcp -w -- /path/to/my-mcp-server --port 8080

# Combine options: watch + preserve ANSI
./target/release/wrap-mcp -w --ansi -- /path/to/my-mcp-server
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
- `format`: Output format (default: `ai`)
  - `ai`: Concise format optimized for AI consumption
  - `full` or `text`: Detailed human-readable format with timestamps and formatting
  - `json`: Raw JSON output

#### `clear_log`
Clears all recorded logs.

#### `restart_wrapped_server`
Restarts the wrapped MCP server while preserving all recorded logs.

This is useful when:
- The wrapped server becomes unresponsive
- You want to reload the wrapped server after updating its code
- You need to reset the wrapped server's state

Note: During restart, client requests will fail temporarily.

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