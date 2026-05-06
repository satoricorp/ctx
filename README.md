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
- **`OPENAI_API_KEY`** — required for extraction and embeddings.
- **Network access to OpenAI** — `ctx` now uses OpenAI for both extraction and embeddings.

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
2. `CTX_IMAGE` (selected context name)
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

## First-run setup

On the first **`ctx init`**, the CLI creates **`~/.ctx/config.json`** and normalizes it to the supported OpenAI-only runtime:

- **`extraction_model`** defaults to **`openai:gpt-5.4-nano`**.
- **`embedding_model`** defaults to **`openai:text-embedding-3-small`**.
- **`OPENAI_API_KEY`** is required. `ctx init` reads it from your shell environment and fails fast if it is missing.

The current runtime uses OpenAI for both extraction and embeddings. Notes on the earlier multi-backend design live in [docs/embedded-models-archive.md](docs/embedded-models-archive.md).

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
| **`OPENAI_API_KEY`** | Required for OpenAI-backed extraction and embeddings. |
| **`CTX_OPENAI_BASE_URL`** | Optional base URL override for OpenAI-compatible APIs and test doubles. |
| **`CTX_SEMANTIC_CHUNK_MERGE_MAX_TOKENS`** | Optional cap (`token_count`, rough chars÷4): merge adjacent semantic chunks until the next would exceed this budget. Default `0` (no merge). |
| **`CTX_EMBEDDING_BATCH_SIZE`** | Max texts per OpenAI embedding batch (default `256`, clamped 1–2048). |
| **`CTX_SEMANTIC_INGEST_CONCURRENCY`** | Parallel semantic **LLM** extractions per document (default `4`). Embeddings run in one batched phase after extraction. |

---

## Dependency notes

- **`helix-db`** is vendored under `vendor/helix/` from [HelixDB/helix-db](https://github.com/HelixDB/helix-db) (with a tiny stdout tweak); the crates.io release is still not what this repo uses.

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
