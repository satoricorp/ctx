# ctx

`ctx` is a local-first context runtime for AI agents.

This repository follows the v7 spec in `.context/attachments/ctx-codex-instructions-v7.md` and currently ships:

- `ctx` for local CLI workflows
- `ctx-server` for the HTTP API
- a managed artifact layout rooted at `~/.ctx` or `CTX_PATH`
- a `helix-db`-backed index directory plus JSON state for the app-level records
- local semantic and procedural indexing, status, list, update, and query flows

## working commands

- `ctx init [name]`
- `ctx add <path> [-c <context>] [--type semantic|procedural]`
- `ctx query <text> [-c <context>] [--type all|semantic|procedural] [--k <n>]`
- `ctx update [-c <context>]`
- `ctx list`
- `ctx status [-c <context>]`
- `ctx-server --port 8080`

## api routes

- `POST /add`
- `POST /query`
- `POST /record`
- `GET /status`

## planned environment variables

Required in some environments:

- `CTX_PATH` overrides the default contexts directory
- `CTX_HOST` overrides the API bind host
- `CTX_PORT` overrides the API port
- `PORT` is honored as a fallback in hosted environments
- `OPENAI_API_KEY` enables future OpenAI-backed extraction and embeddings
- `ANTHROPIC_API_KEY` enables future Anthropic-backed extraction

Optional for deterministic local tests and offline smoke checks:

- `CTX_DISABLE_FASTEMBED=1` forces the lexical hash embedding fallback

## implementation notes

- the current local runtime prefers a `fastembed` model when it is available and falls back to a deterministic hash embedding when it is not yet installed
- publish, pull, auth device flow, and the MCP server are still stubbed and need a follow-up pass
- the published crates.io `helix-db` release did not compile cleanly in this environment, so the repository currently tracks the upstream Git dependency instead
