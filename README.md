# ctx

`ctx` is a local-first context runtime for AI agents.

This repository currently follows the v7 spec in `.context/attachments/ctx-codex-instructions-v7.md` and is being built as a Rust workspace with:

- `ctx` for the CLI
- `ctx-server` for the HTTP API
- a managed artifact layout rooted at `~/.ctx`
- embedded local retrieval primitives with a `helix-db`-backed index directory

## status

The project is scaffolded around the spec's module layout and is intentionally split into small implementation commits.

## planned environment variables

- `CTX_PATH` overrides the default contexts directory
- `CTX_HOST` overrides the API bind host
- `CTX_PORT` overrides the API port
- `PORT` is honored as a fallback in hosted environments
- `OPENAI_API_KEY` enables OpenAI-backed extraction and embeddings
- `ANTHROPIC_API_KEY` enables Anthropic-backed extraction

Additional auth and registry overrides will be documented once the publish/pull contract is finalized.

