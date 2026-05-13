#!/usr/bin/env bash
# Single-platform compile + publish loop for the cinccino VS Code
# extension. Builds cinccino-lsp for *this* host, stages it under
# server/<vscode-target>/, packages a platform-targeted .vsix, and
# publishes to the VS Code Marketplace.
#
# Usage:
#   ./release.sh                  # publish current package.json version
#   ./release.sh patch            # bump 0.1.6 → 0.1.7 then publish
#   ./release.sh minor            # bump 0.1.6 → 0.2.0 then publish
#   ./release.sh --dry-run        # build + package only, skip publish
#   ./release.sh patch --dry-run  # bump + build + package, skip publish
#
# Requires:
#   - cargo (Rust toolchain) — for building cinccino-lsp
#   - node + npm                — for building the extension
#   - VSCE_PAT env var          — set non-interactively in your shell
#                                 (e.g. `read -s VSCE_PAT && export VSCE_PAT`)
#                                 unless --dry-run.
#
# This is a *single-platform* deploy. Users on other platforms will fall
# back to the generic .vsix (which auto-installs the server via cargo).
# For all-platform releases, push a tag to mystiz-miso/cinccino and let
# the GitHub Actions matrix do it.

set -euo pipefail

# ── Args ────────────────────────────────────────────────────────────
BUMP=""
DRY_RUN=0
for arg in "$@"; do
  case "$arg" in
    patch|minor|major) BUMP="$arg" ;;
    --dry-run)         DRY_RUN=1 ;;
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
# Expect cinccino crate at the parent of the extension (one of two layouts):
#   layout A: monorepo  → <root>/cinccino/{Cargo.toml, vscode-extension/}
#   layout B: standalone → <root>/{Cargo.toml, vscode-extension/}
if [[ -f "$EXT_DIR/../Cargo.toml" ]]; then
  CRATE_DIR="$(cd "$EXT_DIR/.." && pwd)"
else
  echo "ERROR: can't find cinccino Cargo.toml at $EXT_DIR/../Cargo.toml" >&2
  exit 1
fi

cd "$EXT_DIR"

# ── Detect host → vscode target ─────────────────────────────────────
case "$(uname -s)" in
  Linux)  OS=linux ;;
  Darwin) OS=darwin ;;
  *)      echo "ERROR: unsupported OS $(uname -s); use the CI workflow instead" >&2; exit 2 ;;
esac
case "$(uname -m)" in
  x86_64|amd64)   ARCH=x64 ;;
  aarch64|arm64)  ARCH=arm64 ;;
  *)              echo "ERROR: unsupported arch $(uname -m)" >&2; exit 2 ;;
esac
TARGET="${OS}-${ARCH}"
echo "→ host platform: $TARGET"

# Rust triples for cargo build --target.
case "$TARGET" in
  linux-x64)    RUST_TARGET=x86_64-unknown-linux-gnu ;;
  linux-arm64)  RUST_TARGET=aarch64-unknown-linux-gnu ;;
  darwin-x64)   RUST_TARGET=x86_64-apple-darwin ;;
  darwin-arm64) RUST_TARGET=aarch64-apple-darwin ;;
esac

# ── Toolchain sanity ────────────────────────────────────────────────
command -v cargo >/dev/null || { echo "ERROR: cargo not on PATH" >&2; exit 3; }
command -v npm   >/dev/null || { echo "ERROR: npm not on PATH"   >&2; exit 3; }
if [[ $DRY_RUN -eq 0 && -z "${VSCE_PAT:-}" ]]; then
  echo "ERROR: VSCE_PAT not set. Run --dry-run, or:" >&2
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

# ── Build cinccino-lsp ──────────────────────────────────────────────
echo "→ building cinccino-lsp (release) for $RUST_TARGET"
(cd "$CRATE_DIR" && cargo build --release --bin cinccino-lsp --target "$RUST_TARGET")
BIN="$CRATE_DIR/target/$RUST_TARGET/release/cinccino-lsp"
[[ -f "$BIN" ]] || { echo "ERROR: built binary not found at $BIN" >&2; exit 4; }

# ── Stage into server/<target>/ ─────────────────────────────────────
STAGE_DIR="$EXT_DIR/server/$TARGET"
mkdir -p "$STAGE_DIR"
cp "$BIN" "$STAGE_DIR/cinccino-lsp"
chmod +x "$STAGE_DIR/cinccino-lsp"
echo "→ staged $(du -h "$STAGE_DIR/cinccino-lsp" | cut -f1) at server/$TARGET/cinccino-lsp"

# ── Bundle the extension ────────────────────────────────────────────
echo "→ bundling extension"
npm install --no-audit --no-fund >/dev/null
npm run compile

# ── Package + (optionally) publish ──────────────────────────────────
VSIX="cinccino-circom-${TARGET}.vsix"
rm -f "$VSIX"
echo "→ packaging $VSIX (vscode target $TARGET)"
npx --yes @vscode/vsce package --no-dependencies --target "$TARGET" --out "$VSIX"

if [[ $DRY_RUN -eq 1 ]]; then
  echo "✓ dry-run complete; .vsix at $EXT_DIR/$VSIX"
  exit 0
fi

echo "→ publishing $VSIX to the VS Code Marketplace"
npx --yes @vscode/vsce publish --no-dependencies --target "$TARGET" --packagePath "$VSIX"
echo "✓ published samueltangz.cinccino-circom@$VERSION for $TARGET"
echo "  Marketplace will route this build to $TARGET users."
echo "  Other platforms still use whatever version was last published for them."
