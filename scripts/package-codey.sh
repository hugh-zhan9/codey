#!/usr/bin/env bash

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${1:-0.0.0-dev}"
OUTPUT_DIR="${OUTPUT_DIR:-$REPO_ROOT/dist}"

detect_target_triple() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"

  case "${os}:${arch}" in
    Darwin:arm64)
      echo "aarch64-apple-darwin"
      ;;
    Darwin:x86_64)
      echo "x86_64-apple-darwin"
      ;;
    Linux:x86_64)
      echo "x86_64-unknown-linux-musl"
      ;;
    Linux:aarch64|Linux:arm64)
      echo "aarch64-unknown-linux-musl"
      ;;
    *)
      echo "Unsupported platform: ${os} ${arch}" >&2
      exit 1
      ;;
  esac
}

TARGET_TRIPLE="$(detect_target_triple)"
STAGE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/codey-stage.XXXXXX")"
PACKAGE_JSON_PATH="$STAGE_DIR/package.json"

cleanup() {
  rm -rf "$STAGE_DIR"
}
trap cleanup EXIT

mkdir -p \
  "$STAGE_DIR/bin" \
  "$STAGE_DIR/vendor/$TARGET_TRIPLE/codex" \
  "$STAGE_DIR/vendor/$TARGET_TRIPLE/path" \
  "$OUTPUT_DIR"

echo "Building codex-cli release binary for $TARGET_TRIPLE..."
cargo build \
  --manifest-path "$REPO_ROOT/codex-rs/Cargo.toml" \
  -p codex-cli \
  --release

cp "$REPO_ROOT/codex-cli/bin/codex.js" "$STAGE_DIR/bin/codex.js"
cp "$REPO_ROOT/codex-rs/target/release/codex" "$STAGE_DIR/vendor/$TARGET_TRIPLE/codex/codex"

if command -v rg >/dev/null 2>&1; then
  cp "$(command -v rg)" "$STAGE_DIR/vendor/$TARGET_TRIPLE/path/rg"
else
  echo "warning: rg not found in PATH; packaged app will rely on host PATH for ripgrep" >&2
  rmdir "$STAGE_DIR/vendor/$TARGET_TRIPLE/path" 2>/dev/null || true
fi

cat > "$PACKAGE_JSON_PATH" <<EOF
{
  "name": "codey",
  "version": "$VERSION",
  "license": "Apache-2.0",
  "bin": {
    "codey": "bin/codex.js"
  },
  "type": "module",
  "engines": {
    "node": ">=16"
  },
  "files": [
    "bin",
    "vendor"
  ]
}
EOF

echo "Packing local npm tarball..."
npm pack "$STAGE_DIR" --pack-destination "$OUTPUT_DIR" >/dev/null

TARBALL_PATH="$OUTPUT_DIR/codey-${VERSION}.tgz"
echo "Packaged codey tarball: $TARBALL_PATH"
echo
echo "Install with:"
echo "  npm install -g \"$TARBALL_PATH\""
