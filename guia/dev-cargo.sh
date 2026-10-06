#!/usr/bin/env bash
set -euo pipefail

# Run cargo for the Rust workspace inside the same toolchain image the release
# Dockerfile uses (rust:1.89-bookworm), without a local Rust install.
# Dependencies and build artifacts live in named Docker volumes so repeated
# runs are incremental and the repository tree stays clean.
#
#   guia/dev-cargo.sh test -p wifi-densepose-sensing-server --lib
#   guia/dev-cargo.sh check -p wifi-densepose-sensing-server

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUST_IMAGE="${RUVIEW_RUST_IMAGE:-rust:1.89-bookworm}"

exec docker run --rm \
  -v "$ROOT_DIR:/repo" \
  -v ruview-cargo-home:/usr/local/cargo/registry \
  -v ruview-cargo-git:/usr/local/cargo/git \
  -v ruview-cargo-target:/cargo-target \
  -e CARGO_TARGET_DIR=/cargo-target \
  -e CARGO_TERM_COLOR=never \
  -e CARGO_INCREMENTAL=1 \
  -w /repo/v2 \
  "$RUST_IMAGE" cargo "$@"
