#!/usr/bin/env bash
# Run in CI right after Swatinem/rust-cache, from src-tauri/.
#
# sherpa-onnx-sys downloads its prebuilt libraries into target/sherpa-onnx-prebuilt/. rust-cache keeps
# only build/, deps/ and .fingerprint/ of target/, so a restored cache has the build script's output
# (which points the linker at that folder) but not the folder. Cargo trusts the output and does not
# run the script again, and the build fails with "could not find native static library
# `sherpa-onnx-c-api`". When the libraries are missing, clean the crate so its build script runs
# and downloads them again.
#
# Usage: bash ../scripts/sherpa-after-cache.sh [comma-separated rust targets]
set -euo pipefail

if ls target/sherpa-onnx-prebuilt/*/lib/*sherpa-onnx-c-api* >/dev/null 2>&1; then
  echo "sherpa-onnx libraries are in target/, nothing to do"
  exit 0
fi

echo "sherpa-onnx libraries are not in target/; cleaning sherpa-onnx-sys so they are downloaded again"
cargo clean -p sherpa-onnx-sys
targets="${1:-}"
for t in ${targets//,/ }; do
  cargo clean -p sherpa-onnx-sys --target "$t"
done
