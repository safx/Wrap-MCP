# Wrap MCP Server

A Model Context Protocol (MCP) server built with Rust using rmcp 0.4.0.

## Features

- Supports stdio and streamable HTTP transport modes

## Usage

### Build
```bash
cargo build --release
```

### Run with stdio transport (default)
```bash
cargo run
```

## Environment Variables

- `WRAP_MCP_TRANSPORT`: Transport mode (`stdio` or `streamable-http`, default: `stdio`)
- `RUST_LOG`: Log level configuration (e.g., `info`, `debug`, `trace`)

