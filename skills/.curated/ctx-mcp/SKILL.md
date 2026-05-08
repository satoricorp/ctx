---
name: ctx-mcp
description: "Use this skill when you need to set up, verify, or troubleshoot the local `ctx` MCP server for agent runtimes or MCP clients such as Claude Desktop."
---

# CTX MCP

## Overview

Use this skill when the goal is to expose `ctx` as MCP tools instead of running one-off CLI commands. It focuses on starting the MCP server, validating it, and wiring it into local MCP clients.

Prefer this skill for:

- starting `ctx mcp`
- checking that the MCP endpoint is alive
- producing client configuration for local MCP consumers
- troubleshooting local `ctx` MCP setup
- explaining when to use MCP instead of the plain CLI
- wiring a client into a shared local memory context

## Preconditions

Before setting up MCP, check:

- `ctx` is installed and runnable
- `OPENAI_API_KEY` is exported if the user expects add/query operations to work through MCP
- the target client can connect to MCP over the transport it expects
- the chosen port is free

Useful checks:

```bash
ctx --help
ctx mcp --help
printenv OPENAI_API_KEY
```

## Start The Server

Run:

```bash
ctx mcp --port 8788
```

This starts the local `ctx` MCP server.

## Verify The Server

You can verify the endpoint responds by initializing it over HTTP:

```bash
curl -X POST http://127.0.0.1:8788/mcp \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"curl","version":"0.1"}}}'
```

Then list tools:

```bash
curl -X POST http://127.0.0.1:8788/mcp \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'
```

Expect tools like:

- `ctx_add`
- `ctx_query`
- `ctx_record`
- `ctx_notes_read`
- `ctx_notes_write`

Use `ctx_notes_write` and `ctx_record` when the client should leave behind durable memory instead of keeping everything trapped in chat history.

## Client Setup Guidance

When the user asks for Claude Desktop or another MCP client, provide configuration in terms of:

- command to start `ctx mcp`
- host and port
- MCP endpoint path `/mcp`
- any required environment such as `OPENAI_API_KEY`

Do not assume every client uses the same transport configuration format. Confirm whether the target client expects:

- a local spawned command
- a local HTTP MCP endpoint
- a remote MCP connector

## When To Prefer MCP

Prefer MCP over the plain CLI when:

- another agent runtime needs `ctx` as callable tools
- the user wants `ctx_add`, `ctx_query`, or `ctx_notes_*` available inside a client
- the workflow should stay inside an MCP-capable tool instead of shell commands
- multiple agents should share one local memory layer on disk

Prefer the CLI skill when the task is simply to run `ctx` commands directly.

## Troubleshooting

- If initialize fails, verify the server is running on the expected port.
- If tools/list is empty or missing expected tools, restart `ctx mcp`.
- If add/query tool calls fail, confirm `OPENAI_API_KEY` is exported in the MCP server environment.
- If a client cannot connect, verify whether it expects local stdio or HTTP MCP and configure accordingly.
