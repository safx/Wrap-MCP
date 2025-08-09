# Wrap-MCP

A transparent MCP (Model Context Protocol) proxy server that sits between MCP clients and MCP servers (wrappees), logging all requests/responses while operating as a transparent proxy.

## Overview

Wrap-MCP wraps existing MCP servers and provides the following features:

- 🔄 Transparent proxy: Appears as a regular MCP server to clients
- 📝 Request/response logging with type-safe storage
- 🔍 Log search and display tools (`show_log`)
- ⚠️ Captures and logs stderr output from the wrappee process
- 🔁 Auto-restart on binary file changes (development mode)
- 🎨 ANSI escape sequence handling for clean logs

## System Architecture

```
MCP Client ◄────────► Wrap-MCP ◄────────► Wrappee (MCP Server)
                          │
                      Log Storage
                   (In-Memory VecDeque)
```

### Internal Structure

- **Modular design**: Separated into `logging`, `proxy`, `tools`, and `wrappee` modules
- **Type-safe logging**: ADT-based `LogEntryContent` enum for compile-time safety
- **Efficient storage**: VecDeque-based ring buffer for O(1) log trimming
- **Async-safe**: Thread-safe operations with RwLock protection

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
  - **Requires absolute path to the wrappee binary**
  - Monitors the wrappee binary file for modifications
  - Automatically restarts the wrapped server when the binary is updated
  - Uses a 2-second debounce to handle multiple rapid file changes during compilation
  - Shows old and new PIDs in logs for verification
  - Useful for development when frequently recompiling the wrapped server

### Environment Variables

- `WRAP_MCP_TRANSPORT`: Transport method (`stdio` or `http`, default: `stdio`)
- `WRAP_MCP_LOGSIZE`: Maximum number of log entries to retain (default: 1000)
- `WRAP_MCP_PROTOCOL_VERSION`: Protocol version to use when initializing the wrapped server (default: `2025.06.18`)
  - This allows compatibility with wrapped servers that require specific protocol versions
  - Example: `WRAP_MCP_PROTOCOL_VERSION="2024.12.01"`
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

# Watch binary file for changes and auto-restart (requires absolute path)
./target/release/wrap-mcp -w -- /path/to/my-mcp-server --port 8080

# Combine options: watch + preserve ANSI (requires absolute path)
./target/release/wrap-mcp -w --ansi -- /path/to/my-mcp-server

# Use a specific protocol version for the wrapped server
WRAP_MCP_PROTOCOL_VERSION="2024.12.01" ./target/release/wrap-mcp -- my-mcp-server
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
  - `text`: Detailed human-readable format with timestamps and formatting
  - `json`: Raw JSON output with full structure

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

### Storage Implementation

- **In-memory storage**: Uses `VecDeque` for efficient FIFO operations
- **Automatic trimming**: O(1) removal of old logs when exceeding `WRAP_MCP_LOGSIZE`
- **Type-safe entries**: Each log entry is strongly typed with ADT enum
- **Thread-safe**: All operations protected by async RwLock
- **Filtering**: Supports filtering by tool name, entry type, and timestamp ranges

### Log Entry Types

- **Request**: Tool invocation with arguments
- **Response**: Successful tool execution results
- **Error**: Failed tool executions with error messages
- **Stderr**: Captured stderr output from wrapped server

### Performance

- Log insertion: O(1)
- Old log removal: O(1) with VecDeque
- Filtering: O(n) where n is the number of logs
- Memory usage: Bounded by `WRAP_MCP_LOGSIZE`

## Development

### Build
```bash
cargo build
```

### Test
```bash
# Run all tests
cargo test

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