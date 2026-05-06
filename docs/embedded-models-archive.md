# Embedded Models Archive

This repo previously supported a more complex runtime:

- OpenAI extraction
- Anthropic extraction
- FastEmbed dense embeddings
- optional Splade sparse retrieval
- embedded local extraction via Gemma GGUF models and `llama.cpp`

That work has been removed from the active runtime for now.

## Why we cut it

- Too many runtime branches for a young project
- first-run behavior was harder to reason about than it needed to be
- cold-start and local model initialization introduced avoidable failure modes
- debugging protocol behavior was getting mixed up with debugging model installation

## What remains active

- OpenAI extraction via Chat Completions
- OpenAI embeddings via the embeddings API
- heuristic extraction fallback when OpenAI extraction fails

## What was built

The removed implementation included:

- config-driven backend selection in `src/install.rs`
- local extraction orchestration in `src/models/llm.rs`
- local embedding and fallback logic in `src/models/embeddings.rs`
- CLI setup flows for local model installs
- docs describing FastEmbed, Splade, Anthropic, and Gemma setup

## If we revisit this later

Review git history before the OpenAI-only simplification and answer these questions first:

1. Do we need local/offline operation, or just lower cost?
2. Do we need multiple providers, or just one provider plus a fallback?
3. Can we keep one retrieval path and one extraction path, instead of a matrix of options?
4. What is the exact cold-start budget we are willing to accept?

If we bring local models back, we should do it behind a clearly separate mode instead of mixing them into the default path.
