# ctx

`ctx` is a local context runtime for AI agents.

Use it when you want an agent to:

- index a codebase or local docs
- keep durable project notes on your machine
- answer questions about that context later

It gives you context layers that lives on your machine, stays inspectable, and can be used via a CLI, MCP, or API.

## What You Get

- Local context stored under `~/.ctx` by default
- Plain markdown notes under each context
- Semantic and procedural indexing
- A CLI for direct use
- An HTTP API for integrations
- An MCP endpoint for agent tooling

## Requirements

- Rust (stable)
- `OPENAI_API_KEY` in your environment

`ctx` currently uses OpenAI for both extraction and embeddings.

## Install

Install the binaries locally:

```bash
git clone <your-fork-or-this-repo>
cd ctx
cargo install --path . --bins
```

Check that your OpenAI key is actually exported:

```bash
printenv OPENAI_API_KEY
```

If that prints nothing:

```bash
export OPENAI_API_KEY=your_key_here
```

After that, use `ctx` directly from your shell.

## Install Skills

For Codex-style runtimes, this repo also ships installable skills under `skills/.curated`.

Install the CLI skill:

```bash
npx skills add https://github.com/satoricorp/ctx --skill ctx-cli -g -y
```

Install the MCP setup skill:

```bash
npx skills add https://github.com/satoricorp/ctx --skill ctx-mcp -g -y
```

After installing a skill, restart Codex so it picks up the new package.

## Quick Start

Create a context:

```bash
ctx init demo
```

Set it as the default context:

```bash
ctx use demo
```

Add files from your machine:

```bash
ctx add README.md
ctx add src
```

Query what you indexed:

```bash
ctx query "how does ctx store notes?"
ctx query "how do I rebuild the index?" --type procedural
```

Refresh after files change:

```bash
ctx update
```

Check status:

```bash
ctx status
```

Run health checks:

```bash
ctx doctor
ctx doctor --fix
```

## How To Think About It

The basic model is:

1. `ctx init <name>` creates a named context.
2. `ctx add <path>` indexes files into that context.
3. `ctx query "<question>"` asks against what was indexed.
4. `ctx update` refreshes the index when files change.
5. `ctx notes` opens the context's local notes directory.

If you only want the shortest possible flow, this is it:

```bash
ctx init demo
ctx use demo
ctx add .
ctx query "what are the main entry points?"
ctx notes
```

## Typical Local Flow

For a fresh project:

```bash
cd /path/to/project
ctx init my-project
ctx use my-project
ctx add .
ctx query "what are the main entry points?"
ctx notes
```

After editing that project later:

```bash
ctx update
ctx query "what changed in the auth flow?"
```

## Notes In A Context

Each context gets a `notes/` directory.

Use `ctx notes` to open it in your file browser.

The important files are:

- `notes/index.md`
- `notes/summary.md`
- `notes/topics/*.md`

Those files are part of the product surface. They are plain markdown and meant to stay readable and editable.

## Run The API

Start the local HTTP server:

```bash
ctx-server --host 127.0.0.1 --port 8787
```

Check health:

```bash
curl http://127.0.0.1:8787/status
curl 'http://127.0.0.1:8787/status?ctx=demo'
```

Query through the API:

```bash
curl -X POST http://127.0.0.1:8787/query \
  -H 'content-type: application/json' \
  -d '{"ctx":"demo","query":"ctx","k":3}'
```

## Run The MCP Server

Start it:

```bash
ctx mcp --port 8788
```

The MCP tools include:

- `ctx_add`
- `ctx_query`
- `ctx_record`
- `ctx_notes_read`
- `ctx_notes_write`

## Common Commands

```bash
ctx init <name>
ctx use <name>
ctx list
ctx add <path>
ctx query "<text>"
ctx update
ctx status
ctx doctor
ctx mcp --port 8788
ctx-server --port 8787
```

## Environment Variables

| Variable | Purpose |
| --- | --- |
| `OPENAI_API_KEY` | Required for extraction and embeddings |
| `CTX_PATH` | Override the default context storage root |
| `CTX_IMAGE` | Default context name when `-c` is omitted |
| `CTX_HOST` | Host for `ctx-server` |
| `CTX_PORT` | Port for `ctx-server` |
| `PORT` | Hosted-environment port fallback |
| `CTX_OPENAI_BASE_URL` | OpenAI-compatible base URL override |
| `CTX_EMBEDDING_BATCH_SIZE` | Embedding batch size |
| `CTX_SEMANTIC_INGEST_CONCURRENCY` | Parallel semantic extraction count |
| `CTX_SEMANTIC_CHUNK_MERGE_MAX_TOKENS` | Optional semantic chunk merge cap |

## Contributing

If you want to fork or extend `ctx`, start here:

```bash
git clone <your-fork>
cd ctx
cargo install --path . --bins
cargo test
```

Useful contributor workflow:

```bash
git checkout -b my-change
cargo test
cargo check
```

If you are changing runtime behavior, update:

- the root `README.md`
- active docs in `docs/` when they describe the changed surface
- tests for the changed behavior

If you are adding or changing the local runtime, the important code is mostly under:

- `src/lib.rs`
- `src/install.rs`
- `src/models/`
- `src/mcp.rs`
- `src/api/`
- `src/notes_update.rs`

## For Contributors Who Want The Old Model Work

The previous multi-backend design was archived on purpose. If you want to revisit that work later, start with:

- [docs/embedded-models-archive.md](docs/embedded-models-archive.md)

## License

`ctx` is licensed under `AGPL-3.0`. See [LICENSE](LICENSE).
