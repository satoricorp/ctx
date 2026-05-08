<p align="center">
<samp>
<font color="#ffffff">┌─────────────────────────┐</font><br>
<font color="#ffffff">│</font><font color="#98ffcc">&nbsp;██████╗████████╗██╗&nbsp;&nbsp;██╗</font><font color="#ffffff">│</font><br>
<font color="#ffffff">│</font><font color="#98ffcc">██╔════╝╚══██╔══╝╚██╗██╔╝</font><font color="#ffffff">│</font><br>
<font color="#ffffff">│</font><font color="#98ffcc">██║&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;██║&nbsp;&nbsp;&nbsp;&nbsp;╚███╔╝&nbsp;</font><font color="#ffffff">│</font><br>
<font color="#ffffff">│</font><font color="#98ffcc">██║&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;██║&nbsp;&nbsp;&nbsp;&nbsp;██╔██╗&nbsp;</font><font color="#ffffff">│</font><br>
<font color="#ffffff">│</font><font color="#98ffcc">╚██████╗&nbsp;&nbsp;&nbsp;██║&nbsp;&nbsp;&nbsp;██╔╝&nbsp;██╗</font><font color="#ffffff">│</font><br>
<font color="#ffffff">│</font><font color="#98ffcc">&nbsp;╚═════╝&nbsp;&nbsp;&nbsp;╚═╝&nbsp;&nbsp;&nbsp;╚═╝&nbsp;&nbsp;╚═╝</font><font color="#ffffff">│</font><br>
<font color="#ffffff">└─────────────────────────┘</font>
</samp>
</p>

<p align="center">
  <font color="#b7b86a">local-first context runtime for agents</font>
</p>

`ctx` is a local memory layer for agents.

Use it when you want to:

- keep durable notes on your machine
- build a shared source of truth across multiple agents
- index code, docs, and working files into that same memory layer
- query what agents already know before repeating work

`ctx` keeps memory local and inspectable. Each context is a folder on disk with plain markdown notes plus retrieval indices. Agents can use it through the CLI, MCP, or an installable skill.

## What You Get

- Local contexts stored under `~/.ctx` by default
- Plain markdown notes under each context
- Searchable semantic and procedural memory
- A shared context that multiple agents can use
- A CLI for direct use
- An MCP endpoint for agent runtimes
- Installable skills for agent runtimes

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

The practical model is simple: each agent runtime can point at the same local `ctx` context and write back into the same notes and records.

## Quick Start

Create a context with a short description:

```bash
ctx init memory --description "Shared notes and memory across my agents"
```

Set it as the default context:

```bash
ctx use memory
```

Open the notes directory:

```bash
ctx notes
```

Add files from your machine into the same context:

```bash
ctx add README.md
ctx add src
```

`ctx add` queues background indexing. Use `ctx status` to watch progress.

Query what the context already knows:

```bash
ctx query "how does ctx store notes?"
ctx query "what decisions have we already made about auth?"
```

Refresh after files change or notes drift:

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

1. `ctx init <name>` creates a named local memory space.
2. `ctx notes` opens the human-editable notes for that memory space.
3. `ctx add <path>` queues code, docs, or files for indexing.
4. `ctx query "<question>"` asks against what is already there.
5. Multiple agents can share the same context through skills or MCP.

If you only want the shortest possible flow, this is it:

```bash
ctx init memory --description "Shared memory across my agents"
ctx use memory
ctx add .
ctx query "what have we already learned about this project?"
ctx notes
```

## Multi-Tool Memory

`ctx` is most useful when more than one agent points at the same context.

For example:

- use the CLI directly to add source material
- use a skill in one runtime to query the same context
- use MCP in another runtime so it can write durable notes and records

Over time, `notes/`, indexed content, and recorded procedures become one local source of truth instead of being scattered across isolated chats.

## Typical Local Flow

For a fresh memory context:

```bash
cd /path/to/project
ctx init my-project --description "Shared project memory for agent work"
ctx use my-project
ctx add .
ctx query "what are the main entry points?"
ctx notes
```

After working on that project later:

```bash
ctx update
ctx query "what changed in the auth flow?"
```

## Notes In A Context

Each context gets a `notes/` directory plus a manifest that tracks the context metadata, including its description.

Use `ctx notes` to open it in your file browser.

The important files are:

- `notes/index.md`
- `notes/summary.md`
- `notes/topics/*.md`

Those files are part of the product surface. They are plain markdown and meant to stay readable and editable. This is the durable layer agents should write into when a fact, decision, workflow, or reminder should survive beyond a single chat.

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

This is the integration surface to use when you want another agent to read from and write to the same local `ctx` memory.

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
