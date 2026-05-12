---
name: ctx
description: "Use this skill when you need to operate the local `ctx` runtime from the command line: initialize contexts, select a default context, queue indexing jobs, query indexed content, inspect status, run doctor, open notes, or start the local MCP server."
---

# CTX CLI

## Overview

`ctx` is a local context runtime for agents. Use this skill when the task is about working with local project context through the `ctx` CLI or exposing `ctx` through MCP.

Prefer this skill for:

- creating or selecting a context
- indexing a repo or local docs
- querying indexed material
- writing durable notes that should survive beyond one chat
- checking job status or health
- opening the context's `notes/` directory
- starting `ctx mcp` for agent runtimes

## Preconditions

Before relying on `ctx`, check:

- `ctx` is installed and on `PATH`
- `OPENAI_API_KEY` is exported
- the user either has a default context (`ctx use <name>`) or you pass `-c <name>`

Useful checks:

```bash
ctx --help
printenv OPENAI_API_KEY
ctx list
ctx status
```

## Core Commands

Create and select a context:

```bash
ctx init demo
ctx use demo
```

Queue indexing:

```bash
ctx add .
ctx add README.md
ctx add src -c demo
```

Remember durable text:

```bash
ctx remember "Use RS256 tokens only"
ctx remember --topic auth "Reject unsigned JWTs"
ctx remember --topic deploy --stdin
```

Use `ctx add` for files and source material. Use `ctx remember` for facts, decisions, summaries, preferences, and reminders that should become durable notes.

Important: `ctx add` is background-only. After starting it, check progress with:

```bash
ctx status
```

Query indexed content:

```bash
ctx query "what are the main entry points?"
ctx query -c demo "how does auth work?"
ctx query --raw "show raw retrieval hits"
```

Refresh after files change:

```bash
ctx update
```

Inspect or repair:

```bash
ctx status
ctx doctor
ctx doctor --fix
```

Open notes:

```bash
ctx notes
ctx notes -c demo
```

Write durable memory into notes when a fact or decision should survive:

- project decisions
- working agreements
- repeated fixes
- user preferences
- next-step reminders that matter beyond the current turn

Prefer `ctx remember --topic <topic> --stdin` for multi-line summaries. It appends to `notes/topics/<topic>.md` and keeps the context manifest in sync.

## Context Selection

Prefer these rules:

- use `ctx use <name>` when the user wants a persistent default context
- use `-c <name>` for one-off commands
- rely on the active/default context only when it is already clearly set

Examples:

```bash
ctx use my-project
ctx query "what changed?"
ctx query -c another-project "what changed?"
```

## MCP

Start the local MCP server with:

```bash
ctx mcp --port 8788
```

Use this when the user wants `ctx` available to another agent runtime or MCP client.

## Working Rules

- Do not treat `ctx add` as synchronous work; always follow with `ctx status` if the user wants completion or progress.
- Prefer `ctx query` default output for clean answers with source attribution. Use `--raw` only when debugging retrieval.
- If the repo changed since the last index, suggest or run `ctx update` before answering important questions.
- When a command needs a specific context and there is no clear default, pass `-c <name>` explicitly.
- Use `ctx notes` instead of telling the user to browse `~/.ctx/...` by hand.
- Treat `notes/` as the long-term memory layer. If a conclusion should survive the session, write it with `ctx remember` instead of leaving it only in the chat transcript.
