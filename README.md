<p align="center">
  <img src="assets/ctx-logo-light.png" alt="CTX" width="360">
</p>

<p align="center">
  <strong>local context for agents</strong>
</p>

**CTX** is local, inspectable memory for AI agents: each *context* is a folder under `~/.ctx` with plain markdown notes plus search indices over files you add. Nothing leaves your machine except the API calls you configure (today: OpenAI for embeddings and extraction). Use one context from the CLI, from MCP, or from installable skills so every agent reads and writes the same source of truth instead of losing it in chat history.

## Install

You need **`OPENAI_API_KEY`** in your environment.

```bash
brew tap satoricorp/tap
brew install satoricorp/tap/ctx
```

Build from source: clone the repo, then `cargo install --path . --bins`.

Release tarballs, Homebrew formula assets, and `.deb` packaging are described in [packaging/README.md](packaging/README.md) for anyone shipping or mirroring installs.

## Use it

```bash
ctx init memory --description "Shared notes across my agents"
ctx use memory
ctx add .                         # index a tree; runs in the background — use ctx status
ctx remember "Facts agents should keep"
ctx query "what do we know about auth?"
ctx notes                         # open editable notes (notes/index.md, summary.md, topics/)
ctx update                        # refresh after files or notes change
ctx doctor                        # ctx doctor --fix for repairs
```

**Mental model:** `init` / `use` pick the memory space. `add` ingests paths. `remember` appends durable markdown. `query` searches notes + indexed content. Wrong context = missing answers — use `ctx use <name>` or `ctx query -c <name>`.

**Agents (Codex-style):** skills live under `skills/.curated`. After install, restart the runtime.

```bash
npx skills add https://github.com/satoricorp/ctx --skill ctx-cli -g -y
npx skills add https://github.com/satoricorp/ctx --skill ctx-mcp -g -y
```

**HTTP API:** `ctx-server --host 127.0.0.1 --port 8787` — e.g. `GET /status`, `POST /query` with JSON `{"ctx":"name","query":"…","k":3}`.

**MCP:** `ctx mcp --port 8788` — tools include `ctx_add`, `ctx_query`, `ctx_record`, `ctx_notes_read`, `ctx_notes_write`.

## Environment (optional)

| Variable | Purpose |
| --- | --- |
| `OPENAI_API_KEY` | Required for extraction and embeddings |
| `CTX_PATH` | Context storage root (default `~/.ctx`) |
| `CTX_IMAGE` | Default context when `-c` is omitted |
| `CTX_HOST` / `CTX_PORT` | `ctx-server` bind |
| `PORT` | Port fallback in hosted environments |
| `CTX_OPENAI_BASE_URL` | OpenAI-compatible API base URL |
| `CTX_EMBEDDING_BATCH_SIZE` | Embedding batch size |
| `CTX_SEMANTIC_INGEST_CONCURRENCY` | Parallel semantic extraction |
| `CTX_SEMANTIC_CHUNK_MERGE_MAX_TOKENS` | Optional chunk merge cap |

*Why keep this table here:* people self-hosting or tuning ingestion should not have to read the source to discover knobs.

## Contributing

```bash
git clone <your-fork>
cd ctx
cargo install --path . --bins
cargo test
```

Change runtime behavior → update this README, relevant `docs/`, and tests. Main Rust areas: `src/lib.rs`, `src/install.rs`, `src/models/`, `src/mcp.rs`, `src/api/`, `src/notes_update.rs`.

Archived multi-backend design notes: [docs/embedded-models-archive.md](docs/embedded-models-archive.md) — *kept so contributors do not duplicate old work or ask “where did it go?”*

## License

AGPL-3.0 — see [LICENSE](LICENSE).
