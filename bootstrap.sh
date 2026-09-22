#!/usr/bin/env bash
# Installs the pinned engine tools: DuckDB (python) and Miller (record-level edits). Idempotent.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
MILLER_VERSION="6.21.0"
python3 -m pip install --quiet --break-system-packages -r requirements.txt 2>/dev/null \
  || python3 -m pip install --quiet -r requirements.txt
if ! command -v mlr >/dev/null || [ "$(mlr --version | awk '{print $2}')" != "$MILLER_VERSION" ]; then
  mkdir -p "$HOME/.local/bin"
  tmp="$(mktemp -d)"
  curl -sL "https://github.com/johnkerl/miller/releases/download/v${MILLER_VERSION}/miller-${MILLER_VERSION}-linux-amd64.tar.gz" | tar xz -C "$tmp"
  cp "$tmp"/miller-*/mlr "$HOME/.local/bin/mlr"
  echo "Installed Miller ${MILLER_VERSION} to ~/.local/bin (make sure it is on PATH)."
fi
python3 -c "import duckdb; print('duckdb', duckdb.__version__)"
mlr --version
