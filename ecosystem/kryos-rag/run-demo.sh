#!/usr/bin/env bash
# Live kryos-rag demo. Requires an API key in the environment:
#   NVIDIA_API_KEY=nvapi-...   ./run-demo.sh    # NVIDIA NIM (OpenAI-compatible)
#   ANTHROPIC_API_KEY=sk-ant-... ./run-demo.sh  # Claude
#
# Needs a Kryos toolchain whose stdlib can do HTTPS POST (the runtime lib must
# include the http_request->reqwest path). The pinned 1.0.0-beta.1+ toolchain
# does. The CI-safe tests (tests/test_rag.kry) need no key and no network.
set -euo pipefail
cd "$(dirname "$0")"

if [ -z "${NVIDIA_API_KEY:-}" ] && [ -z "${ANTHROPIC_API_KEY:-}" ]; then
  echo "Set NVIDIA_API_KEY (or ANTHROPIC_API_KEY) first, then: ./run-demo.sh" >&2
  exit 1
fi

exec kryos run src/main.kry
