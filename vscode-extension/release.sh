#!/usr/bin/env bash
# Compile + deploy the cinccino VS Code extension from this machine.
#
# Modes (mutually exclusive):
#   (default)     Build for the *current* host platform only, bundle the
#                 binary into a platform-targeted .vsix, publish it.
#   --generic     Publish an untargeted .vsix with NO bundled binary —
#                 every user falls through to the auto-cargo-install
#                 path. Useful as a one-shot "ship to every platform at
#                 once" when you don't have CI set up.
#   --targets X,Y[,…]
#                 Comma-separated list of vscode targets to build and
#                 publish. Each target must be buildable from this host
#                 (linux-x64 native; linux-arm64 via `cross`; win32-x64
#                 via mingw if installed; darwin-* requires macOS).
#                 Example: --targets linux-x64,linux-arm64
#
# Other flags (combinable with all modes):
#   patch|minor|major   Bump package.json before publishing.
#   --dry-run           Build + package, skip the publish step.
#   -h, --help          Show this message.
#
# Environment:
#   VSCE_PAT  Required for publish (not for --dry-run). Set non-
#             interactively: `read -s VSCE_PAT && export VSCE_PAT`.
#
# Pragmatic "publish to everyone, no CI" recipe:
#   ./release.sh patch                   # ship native bundle for linux-x64
#   ./release.sh --generic               # ship untargeted .vsix for everyone else
# After both, Marketplace serves the bundle to linux-x64 users (zero
# install) and the generic vsix to all other platforms (cargo fallback).

set -euo pipefail

# ── Args ────────────────────────────────────────────────────────────
BUMP=""
DRY_RUN=0
MODE="host"
TARGETS=""
for arg in "$@"; do
  case "$arg" in
    patch|minor|major) BUMP="$arg" ;;
    --dry-run)         DRY_RUN=1 ;;
    --generic)         MODE="generic" ;;
    --targets=*)       MODE="targets"; TARGETS="${arg#--targets=}" ;;
    --targets)         echo "ERROR: --targets requires =list (no space)" >&2; exit 64 ;;
    -h|--help)
      sed -n '2,/^set -/p' "$0" | sed 's/^# \{0,1\}//;/^set -/d'
      exit 0
      ;;
    *)
      echo "Unknown arg: $arg" >&2
      echo "Run with --help to see usage." >&2
      exit 64
      ;;
  esac
done

# ── Locate ourselves ────────────────────────────────────────────────
EXT_DIR="$(cd "$(dirname "$0")" && pwd)"
if [[ -f "$EXT_DIR/../Cargo.toml" ]]; then
  CRATE_DIR="$(cd "$EXT_DIR/.." && pwd)"
else
  echo "ERROR: can't find cinccino Cargo.toml at $EXT_DIR/../Cargo.toml" >&2
  exit 1
fi
cd "$EXT_DIR"

# ── Detect host (used by default + as the "native" target in --targets) ──
case "$(uname -s)" in
  Linux)  HOST_OS=linux ;;
  Darwin) HOST_OS=darwin ;;
  *)      HOST_OS="" ;;
esac
case "$(uname -m)" in
  x86_64|amd64)   HOST_ARCH=x64 ;;
  aarch64|arm64)  HOST_ARCH=arm64 ;;
  *)              HOST_ARCH="" ;;
esac
HOST_TARGET=""
[[ -n "$HOST_OS" && -n "$HOST_ARCH" ]] && HOST_TARGET="${HOST_OS}-${HOST_ARCH}"

# Map vscode-target → rust triple. Used by build_one().
rust_triple_for() {
  case "$1" in
    linux-x64)    echo "x86_64-unknown-linux-gnu" ;;
    linux-arm64)  echo "aarch64-unknown-linux-gnu" ;;
    darwin-x64)   echo "x86_64-apple-darwin" ;;
    darwin-arm64) echo "aarch64-apple-darwin" ;;
    win32-x64)    echo "x86_64-pc-windows-msvc" ;;
    *)            echo "" ;;
  esac
}

# Are we expected to use `cross` to build $1 from this host?
needs_cross() {
  [[ "$1" != "$HOST_TARGET" ]]
}

# ── Toolchain sanity ────────────────────────────────────────────────
command -v npm >/dev/null || { echo "ERROR: npm not on PATH" >&2; exit 3; }
if [[ "$MODE" != "generic" ]]; then
  command -v cargo >/dev/null || { echo "ERROR: cargo not on PATH (source ~/.cargo/env)" >&2; exit 3; }
fi
if [[ $DRY_RUN -eq 0 && -z "${VSCE_PAT:-}" ]]; then
  echo "ERROR: VSCE_PAT not set. Either --dry-run or:" >&2
  echo "       read -s VSCE_PAT && export VSCE_PAT" >&2
  exit 3
fi

# ── Optional version bump ───────────────────────────────────────────
if [[ -n "$BUMP" ]]; then
  echo "→ bumping version ($BUMP)"
  npm version "$BUMP" --no-git-tag-version >/dev/null
fi
VERSION=$(node -p "require('./package.json').version")
echo "→ extension version: $VERSION"

# ── npm deps + bundle (shared across all targets) ───────────────────
echo "→ npm install + bundle"
npm install --no-audit --no-fund >/dev/null
npm run compile

# ── Helpers ─────────────────────────────────────────────────────────

# Build cinccino-lsp for a vscode target. Stage into server/<target>/.
build_one() {
  local target="$1"
  local rust_triple
  rust_triple=$(rust_triple_for "$target")
  [[ -n "$rust_triple" ]] || { echo "ERROR: unknown vscode target $target" >&2; exit 5; }

  echo "→ building cinccino-lsp for $target ($rust_triple)"
  local builder=cargo
  if needs_cross "$target"; then
    command -v cross >/dev/null || {
      echo "ERROR: building $target from $HOST_TARGET needs \`cross\`." >&2
      echo "       Install with: cargo install cross --locked" >&2
      exit 6
    }
    builder=cross
  fi
  (cd "$CRATE_DIR" && "$builder" build --release --bin cinccino-lsp --target "$rust_triple")

  local exe="cinccino-lsp"
  [[ "$target" == "win32-x64" ]] && exe="cinccino-lsp.exe"
  local src="$CRATE_DIR/target/$rust_triple/release/$exe"
  [[ -f "$src" ]] || { echo "ERROR: built binary not found at $src" >&2; exit 4; }

  local stage="$EXT_DIR/server/$target"
  rm -rf "$stage"
  mkdir -p "$stage"
  cp "$src" "$stage/$exe"
  [[ "$target" != "win32-x64" ]] && chmod +x "$stage/$exe"
  echo "  staged at server/$target/$exe"
}

# Package + publish one vsix. If $1 is empty, produces an untargeted vsix.
package_publish() {
  local target="${1:-}"
  local args=()
  local vsix
  if [[ -n "$target" ]]; then
    args=(--target "$target")
    vsix="cinccino-circom-${target}.vsix"
  else
    vsix="cinccino-circom-generic.vsix"
  fi
  rm -f "$vsix"

  echo "→ packaging ${vsix}"
  npx --yes @vscode/vsce package --no-dependencies "${args[@]}" --out "$vsix"

  if [[ $DRY_RUN -eq 1 ]]; then
    echo "  dry-run: $EXT_DIR/$vsix"
    return
  fi
  echo "→ publishing $vsix"
  npx --yes @vscode/vsce publish --no-dependencies "${args[@]}" --packagePath "$vsix"
  echo "  ✓ published"
}

# ── Dispatch on mode ────────────────────────────────────────────────
case "$MODE" in
  host)
    [[ -n "$HOST_TARGET" ]] || { echo "ERROR: unsupported host $(uname -s)/$(uname -m)" >&2; exit 2; }
    echo "→ host target: $HOST_TARGET"
    build_one "$HOST_TARGET"
    package_publish "$HOST_TARGET"
    ;;
  generic)
    # Make sure no stale binary leaks into the generic .vsix.
    rm -rf "$EXT_DIR/server"
    echo "→ generic mode: no bundled binary; relies on the extension's"
    echo "  cargo-install fallback for every user."
    package_publish ""
    ;;
  targets)
    IFS=',' read -r -a TGTS <<<"$TARGETS"
    [[ ${#TGTS[@]} -gt 0 ]] || { echo "ERROR: --targets list empty" >&2; exit 64; }
    rm -rf "$EXT_DIR/server"
    for t in "${TGTS[@]}"; do
      build_one "$t"
    done
    # Each target gets its own .vsix so vsce can route by platform.
    for t in "${TGTS[@]}"; do
      # vsce package looks at the whole server/ tree; trim to just the
      # current target's binary before each package step.
      tmp_other=$(mktemp -d)
      shopt -s nullglob
      for other_dir in "$EXT_DIR"/server/*/; do
        other_name=$(basename "$other_dir")
        [[ "$other_name" == "$t" ]] && continue
        mv "$other_dir" "$tmp_other/"
      done
      shopt -u nullglob

      package_publish "$t"

      # Restore for the next target's iteration.
      for restored in "$tmp_other"/*/; do
        mv "$restored" "$EXT_DIR/server/"
      done
      rmdir "$tmp_other"
    done
    ;;
esac

echo
echo "Done. Marketplace listing:"
echo "  https://marketplace.visualstudio.com/items?itemName=samueltangz.cinccino-circom"
