# ctx

**ctx** is a local-first context runtime for AI agents. It indexes project text (semantic and procedural), stores artifacts under your control, and exposes the same behavior through a CLI, an HTTP API, and an MCP server so tools like editors and agents can share one context layer.

At a glance:

- **`ctx`** — initialize contexts, add files, run queries, refresh indexes, and inspect status from the terminal.
- **`ctx-server`** — serve the HTTP API (add, query, record, status) for integrations or remote use.
- **`ctx mcp`** — expose an MCP endpoint (for example `/mcp` on a chosen port) for MCP-aware clients.

Data lives under **`~/.ctx`** by default (override with **`CTX_PATH`**). This repo follows the v7 spec in `.context/attachments/ctx-codex-instructions-v7.md` for deeper behavior and contracts.

---

## Requirements

- **Rust** (stable), recent enough for the 2021 edition — install via [rustup](https://rustup.rs/).
- **Optional API keys** — `OPENAI_API_KEY` and/or `ANTHROPIC_API_KEY` for cloud-backed model selection when you run `ctx init` (see [First-run model setup](#first-run-model-setup)).
- **Network** on first run if you use local models — FastEmbed pulls embedding assets, and interactive local extraction setup can pull a GGUF model for `llama.cpp`.

---

## Build and run locally

From a clone of this repository:

```bash
cargo build --release
```

Binaries are built to `target/release/ctx` and `target/release/ctx-server`. Add that directory to your `PATH`, or run them by path.

```bash
./target/release/ctx --help
./target/release/ctx-server --help
```

---

## Quick start

Typical flow:

1. **`ctx init [name]`** — create a context and config under `~/.ctx` (or `CTX_PATH`).
2. **`ctx use <context>`** — set the default context in `~/.ctx/config.json` so `-c` is optional.
3. **`ctx add <path> [-c <context>] [--type semantic|procedural] [--with-content] [-v|--verbose]`** — index files or trees. Large runs can prompt for **sync vs background** (see [Indexing jobs](#indexing-jobs-interactive--background)); use **`--yes`** / **`--no-interactive`** for scripts, **`--dry-run`** to preview work and a rough time estimate, **`--background`** (Unix) to detach a worker.
4. **`ctx query <text> [-c <context>] [--type all|semantic|procedural] [--k <n>]`** — search indexed content.
5. **`ctx update [-c <context>] [-v|--verbose]`** — refresh indexes when files change (same interactive/background flags as **`ctx add`**).
6. **`ctx status [-c <context>]`** — see counts and dirty/pending state, plus **active indexing job** progress when a background or sync run has written `run/active.json` under the context.

Other useful commands:

- **`ctx list`** — list contexts.
- **`ctx doctor [-c <context>] [--fix] [--json]`** — deep health checks + optional repairs.
- **`ctx-server --port 8080`** — start the API (combine with `CTX_HOST` / `CTX_PORT` / `PORT` as needed).
- **`ctx mcp --port 3000`** — MCP streamable HTTP server for local tooling.

`ctx publish` and `ctx pull` are reserved for future artifact sync; they are not implemented yet.

Batch ingestion behavior (`ctx add <dir>` and `ctx update`) now always prints a one-line summary at
the end (`decoded / skipped ...`). Per-file skip diagnostics are hidden by default and shown only
with `-v` / `--verbose`.

Context resolution for commands that accept `-c` follows:

1. explicit `-c/--context`
2. `CTX_IMAGE` (legacy env alias for selected context name)
3. config default from `ctx use <context>`
4. current directory name inference

`ctx status` is intentionally quick status-only. Deep integrity and repair flows live under
`ctx doctor`.

---

## Indexing jobs (interactive + background)

For **`ctx add`** on a directory and for **`ctx update`**, ctx can **plan** work up front (how many files need indexing, approximate size), show a **rough** duration estimate, and ask how to run the job when the estimate is large (defaults: about **60 seconds** or **10+ files**).

- **Sync** — indexes in the current process; `run/active.json` under the context is updated as files complete so **`ctx status`** can show **percent done**, current file, and a very rough **ETA** once `~/.ctx/indexing-stats.json` has learned from past runs.
- **Background (Unix)** — starts a **detached** worker (`setsid` so closing the terminal does not send SIGHUP). Logs go to **`run/job-<id>.log`**; progress is still visible via **`ctx status`**.

**Resume:** each file commits manifest and index state when it finishes. If a job stops (**sleep**, laptop closed, API error), run **`ctx add …`** or **`ctx update`** again; already-indexed files are skipped.

**Sleep vs disconnect:** background mode helps when the **terminal session** ends. **macOS sleep** still suspends the machine and can stall or time out network calls; for long overnight indexing use something like **`caffeinate -dims ctx add …`** (or the same wrapping **`ctx update`**).

**Windows:** **`--background`** is not supported; use sync mode or run under WSL.

---

## First-run model setup

On the first **`ctx init`**, the CLI creates **`~/.ctx/config.json`** and aligns the local model cache with your environment:

- If **`OPENAI_API_KEY`** is set, config prefers **`openai:gpt-5.4-nano`** for extraction (Chat Completions with **`reasoning_effort: low`** for speed).
- If **`ANTHROPIC_API_KEY`** is set, config prefers **`anthropic:claude-sonnet-4-6`** for extraction.
- Otherwise, you are prompted for the local extraction tier and optional SPLADE setup.
- Default local assets are warmed through **fastembed** (for example **`all-MiniLM-L6-v2`** and **`BGERerankerBase`**).
- If you choose **`gemma4-e4b`** or **`gemma4-26b-a4b`** in a real terminal, `ctx` now preinstalls the matching GGUF model and uses embedded **`llama.cpp`** inference for semantic and procedural extraction.
- Headless first-run setup keeps the chosen local extraction model in config, but defers the GGUF download until the first extraction request.
- **`CTX_DISABLE_FASTEMBED=1`** skips FastEmbed downloads and keeps a deterministic dense fallback (for **`fastembed:`** embeddings only; OpenAI embeddings ignore this flag).
- **`CTX_SKIP_LLAMA_DOWNLOAD=1`** disables automatic GGUF downloads and forces extraction to fall back to heuristics if the local model is missing.

**Cloud models:** With **`OPENAI_API_KEY`** set, **`openai:…`** extraction models use the OpenAI Chat Completions API (JSON responses). With **`ANTHROPIC_API_KEY`** set, **`anthropic:…`** models use the Anthropic Messages API. First-time setup with an OpenAI key also defaults **`embedding_model`** to **`openai:text-embedding-3-large`**; you can set **`embedding_model`** to **`openai:text-embedding-3-small`** or keep **`fastembed:…`** if you prefer. Switching embedding models after indexing a context requires re-indexing for consistent vector search.

---

## HTTP API and health checks

Routes include:

- **`POST /add`**, **`POST /query`**, **`POST /record`**
- **`GET /status`**

### `/status` contract

- **`GET /status`** — liveness/readiness for containers and orchestration (small response).
- **`GET /status?ctx=<name>`** — per-context snapshot: indexed/dirty/pending counts, chunk/entity/relation/procedure counts, and active model config.

---

## Environment variables

| Variable | Role |
| -------- | ---- |
| **`CTX_PATH`** | Overrides the default contexts directory (`~/.ctx`). |
| **`CTX_IMAGE`** | Legacy alias for selected context name when `-c` is omitted. |
| **`CTX_HOST`** | API bind host for `ctx-server`. |
| **`CTX_PORT`** | API port for `ctx-server`. |
| **`PORT`** | Fallback port in hosted environments. |
| **`OPENAI_API_KEY`** | Enables OpenAI-backed extraction model selection in config. |
| **`ANTHROPIC_API_KEY`** | Enables Anthropic-backed extraction model selection in config. |
| **`CTX_DISABLE_FASTEMBED=1`** | Skips FastEmbed downloads; uses deterministic dense fallback. |
| **`CTX_SKIP_LLAMA_DOWNLOAD=1`** | Skips automatic GGUF downloads for local extraction models. |
| **`CTX_LLAMA_MAX_TOKENS`** | Optional local llama generation cap (default `768`). |
| **`CTX_LLAMA_TIMEOUT_MS`** | Optional local llama timeout in milliseconds (default `45000`). |
| **`CTX_LLAMA_N_CTX`** | Optional local llama context window (default `8192`). |
| **`CTX_SEMANTIC_CHUNK_MERGE_MAX_TOKENS`** | Optional cap (`token_count`, rough chars÷4): merge adjacent semantic chunks until the next would exceed this budget. Default `0` (no merge). |
| **`CTX_EMBEDDING_BATCH_SIZE`** | Max texts per OpenAI / FastEmbed embedding batch (default `256`, clamped 1–2048). |
| **`CTX_SEMANTIC_INGEST_CONCURRENCY`** | Parallel semantic **LLM** extractions per document (default `4`). Embeddings run in one batched phase after extraction. |

---

## Dependency notes

- **`helix-db`** is vendored under `vendor/helix/` from [HelixDB/helix-db](https://github.com/HelixDB/helix-db) (with a tiny stdout tweak); the crates.io release is still not what this repo uses.
- **`llama-cpp-2`** and **`encoding_rs`** are now included directly for embedded local extraction.

---

## Deployment: AWS ECS via `infra/`

This repo includes CDK infrastructure in `infra/` for AWS ECS Fargate deployment.

### First-time manual deploy

```bash
cd infra
bun install
bunx cdk bootstrap
bunx cdk deploy --require-approval never
```

After deploy, check the stack output `LoadBalancerDNS` and verify:

```bash
curl "http://<LoadBalancerDNS>/status"
```

### Continuous deploy with GitHub Actions

`.github/workflows/deploy.yml` runs on pushes to `main` and performs:

1. AWS auth using GitHub OIDC
2. `bunx cdk deploy --require-approval never` from `infra/`

That CDK deploy rebuilds and pushes the Docker image, then rolls the ECS service.

Set these in your GitHub repository:

| Type | Name | Purpose |
| ---- | ---- | ------- |
| Secret | `AWS_ROLE_TO_ASSUME` | IAM role ARN trusted by GitHub OIDC for deploy permissions |
| Variable | `AWS_REGION` | AWS region for deploys (for example `us-east-1`) |

### Self-hosted without GitHub Actions

You can run the same `infra/` CDK commands from your own CI runner or local operator machine with AWS credentials.

---

## License

`ctx` is licensed under the GNU Affero General Public License v3.0 (`AGPL-3.0`).
See [LICENSE](LICENSE) for the full license text.
