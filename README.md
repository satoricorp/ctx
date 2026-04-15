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
2. **`ctx add <path> [-c <context>] [--type semantic|procedural]`** — index files or trees.
3. **`ctx query <text> [-c <context>] [--type all|semantic|procedural] [--k <n>]`** — search indexed content.
4. **`ctx update [-c <context>]`** — refresh indexes when files change.
5. **`ctx status [-c <context>]`** — see counts and dirty/pending state.

Other useful commands:

- **`ctx list`** — list contexts.
- **`ctx-server --port 8080`** — start the API (combine with `CTX_HOST` / `CTX_PORT` / `PORT` as needed).
- **`ctx mcp --port 3000`** — MCP streamable HTTP server for local tooling.

`ctx publish` and `ctx pull` are reserved for future artifact sync; they are not implemented yet.

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

---

## Dependency notes

- **`helix-db`** stays on the upstream Git dependency by decision; the crates.io release is still not the path this repo uses.
- **`llama-cpp-2`** and **`encoding_rs`** are now included directly for embedded local extraction.

---

## Deployment: infra submodule at `infra/deploy`

Platform-specific deploy logic (Terraform, Fly, Kubernetes, and so on) often lives in a **separate repository**. This repo expects an optional checkout at **`infra/deploy`** with two entrypoints used by **`.github/workflows/deploy.yml`**:

- **`infra/deploy/scripts/deploy-infra.sh`** — provision or update infrastructure (optional; triggered via workflow dispatch).
- **`infra/deploy/scripts/deploy-app.sh`** — deploy the application (receives **`IMAGE_TAG`**, e.g. `sha-<git-sha>`).

### Add the submodule

Use your real infra remote (HTTPS or SSH):

```bash
git submodule add <your-infra-repo-url> infra/deploy
git commit -m "Add infra/deploy submodule for ctx deployment"
```

Your infra repo should expose the scripts above (and any helpers) under `scripts/`. Clone with submodules elsewhere:

```bash
git clone --recurse-submodules <this-repo-url>
# or after a normal clone:
git submodule update --init --recursive
```

### GitHub Actions

The deploy workflow checks for **`infra/deploy/.git`** (a normal submodule has a `.git` file or directory there). It then uses repository secrets:

| Secret | Purpose |
| ------ | ------- |
| **`INFRA_REPO`** | GitHub repo for the infra checkout (for example **`owner/ctx-infra`**). |
| **`DEPLOY_SSH_KEY`** | SSH private key with read access to **`INFRA_REPO`**. |

The workflow checks out **`INFRA_REPO`** into **`infra/deploy`** during the job when deploy is enabled. The **`actions/checkout`** step uses **`submodules: recursive`** so a committed **`infra/deploy`** submodule is populated on the runner and the deploy job can detect it.

Manual runs can enable **“deploy infra”** via **`workflow_dispatch`** when you need **`deploy-infra.sh`**.

### Self-hosted without GitHub Actions

You can still follow the same contract: build **`ctx-server`**, ship it in an image or binary, mount persistent storage at **`CTX_PATH`**, set secrets for API keys, and use **`GET /status`** for health. The v7 spec includes a minimal Docker-oriented example under **deployment** in `.context/attachments/ctx-codex-instructions-v7.md`.

---

## License

`ctx` is licensed under the GNU Affero General Public License v3.0 (`AGPL-3.0`).
See [LICENSE](LICENSE) for the full license text.
