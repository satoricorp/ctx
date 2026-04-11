#!/usr/bin/env bash
set -euo pipefail

[ -n "${OPENAI_API_KEY:-}" ] && export OPENAI_API_KEY
[ -n "${ANTHROPIC_API_KEY:-}" ] && export ANTHROPIC_API_KEY

exec "$@"

