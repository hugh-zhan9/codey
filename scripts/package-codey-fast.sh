#!/usr/bin/env bash

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORKSPACE_CARGO_TOML="$REPO_ROOT/codex-rs/Cargo.toml"
OUTPUT_DIR="${OUTPUT_DIR:-$REPO_ROOT/dist}"
TARGET_DIR="${CARGO_TARGET_DIR:-}"
INSTALL_AFTER_PACK=0
USE_RELEASE=0
VERSION=""
TARGET_DIR_WAS_DEFAULT=0

usage() {
  cat <<'EOF'
Usage:
  scripts/package-codey-fast.sh [--install] [--release] [--version <version>]

Options:
  --install            Install the generated tarball globally with npm.
  --release            Build with cargo --release. Default is the faster dev build.
  --version <version>  Override the package version. Defaults to the current codex-cli cargo version.
  -h, --help           Show this help text.

Environment:
  OUTPUT_DIR           Directory for the generated tarball. Default: <repo>/dist
  CARGO_TARGET_DIR     Cargo target directory. Default: a unique temp directory per run
EOF
}

read_package_version() {
  (
    cd "$REPO_ROOT/codex-rs"
    cargo pkgid -p codex-cli 2>/dev/null
  ) | sed -n 's/.*#codex-cli@\(.*\)$/\1/p'
}

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
      echo "x86_64-unknown-linux-gnu"
      ;;
    Linux:aarch64|Linux:arm64)
      echo "aarch64-unknown-linux-gnu"
      ;;
    *)
      echo "Unsupported platform: ${os} ${arch}" >&2
      exit 1
      ;;
  esac
}

stop_conflicting_builds() {
  local current_pid shell_pid pids
  current_pid="$$"
  shell_pid="${BASHPID:-$$}"

  pids="$(
    pgrep -f "cargo build .*--manifest-path $WORKSPACE_CARGO_TOML .* -p codex-cli .* --bin codex" || true
  )"
  pids+=$'\n'"$(
    pgrep -f "bash .*scripts/package-codey-fast.sh|bash .*scripts/package-codey.sh|scripts/package-codey-fast.sh|scripts/package-codey.sh" || true
  )"

  printf '%s\n' "$pids" \
    | awk 'NF { print $1 }' \
    | sort -u \
    | while read -r pid; do
        if [[ -z "$pid" || "$pid" == "$current_pid" || "$pid" == "$shell_pid" ]]; then
          continue
        fi
        if ps -p "$pid" >/dev/null 2>&1; then
          echo "Stopping previous packaging process $pid ..."
          kill "$pid" >/dev/null 2>&1 || true
        fi
      done
}

run_cargo_build() {
  if [[ ${#BUILD_ARGS[@]} -gt 0 ]]; then
    CARGO_TARGET_DIR="$TARGET_DIR" cargo build \
      --manifest-path "$WORKSPACE_CARGO_TOML" \
      -p codex-cli \
      --bin codex \
      "${BUILD_ARGS[@]}"
  else
    CARGO_TARGET_DIR="$TARGET_DIR" cargo build \
      --manifest-path "$WORKSPACE_CARGO_TOML" \
      -p codex-cli \
      --bin codex
  fi
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --install)
      INSTALL_AFTER_PACK=1
      shift
      ;;
    --release)
      USE_RELEASE=1
      shift
      ;;
    --version)
      if [[ $# -lt 2 ]]; then
        echo "--version requires a value" >&2
        exit 1
      fi
      VERSION="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ -z "$VERSION" ]]; then
  VERSION="$(read_package_version)"
fi

if [[ -z "$VERSION" ]]; then
  echo "Failed to determine codex-cli version from cargo metadata" >&2
  exit 1
fi

if [[ -z "$TARGET_DIR" ]]; then
  TARGET_DIR="$(mktemp -d "${TMPDIR:-/tmp}/codey-pack-target.XXXXXX")"
  TARGET_DIR_WAS_DEFAULT=1
fi

TARGET_TRIPLE="$(detect_target_triple)"
PROFILE_DIR="debug"
BUILD_ARGS=()

if [[ $USE_RELEASE -eq 1 ]]; then
  PROFILE_DIR="release"
  BUILD_ARGS+=(--release)
fi

STAGE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/codey-stage.XXXXXX")"
PACKAGE_JSON_PATH="$STAGE_DIR/package.json"

cleanup() {
  rm -rf "$STAGE_DIR"
  if [[ $TARGET_DIR_WAS_DEFAULT -eq 1 ]]; then
    rm -rf "$TARGET_DIR"
  fi
}
trap cleanup EXIT

mkdir -p \
  "$STAGE_DIR/bin" \
  "$STAGE_DIR/vendor/$TARGET_TRIPLE/codex" \
  "$STAGE_DIR/vendor/$TARGET_TRIPLE/path" \
  "$OUTPUT_DIR"

stop_conflicting_builds

echo "Building codex-cli (${PROFILE_DIR}) into $TARGET_DIR ..."
if ! run_cargo_build; then
  echo "Initial build failed. Clearing $TARGET_DIR and retrying once ..."
  rm -rf "$TARGET_DIR"
  mkdir -p "$TARGET_DIR"
  run_cargo_build
fi

BINARY_PATH="$TARGET_DIR/$PROFILE_DIR/codex"
if [[ ! -x "$BINARY_PATH" ]]; then
  echo "Expected built binary at $BINARY_PATH" >&2
  exit 1
fi

cp "$REPO_ROOT/codex-cli/bin/codex.js" "$STAGE_DIR/bin/codex.js"
cp "$BINARY_PATH" "$STAGE_DIR/vendor/$TARGET_TRIPLE/codex/codex"

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
rm -f "$OUTPUT_DIR/codey-$VERSION.tgz"
npm pack "$STAGE_DIR" --pack-destination "$OUTPUT_DIR" >/dev/null

TARBALL_PATH="$OUTPUT_DIR/codey-$VERSION.tgz"
echo "Packaged codey tarball: $TARBALL_PATH"

if [[ $INSTALL_AFTER_PACK -eq 1 ]]; then
  echo "Installing $TARBALL_PATH ..."
  npm install -g "$TARBALL_PATH" >/dev/null
  echo "Installed codey globally."
fi

echo
echo "Usage:"
echo "  codey --version"
echo "  codey"
