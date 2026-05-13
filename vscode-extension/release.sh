#!/usr/bin/env bash
# Build + publish a generic (untargeted) .vsix for the cinccino VS Code
# extension. The .vsix contains no platform binary; on first activation
# the extension auto-installs cinccino-lsp via cargo on every user's
# machine. This is a no-CI, one-command publish that covers every
# platform at once.
#
# Usage:
#   ./release.sh            # publish current package.json version
#   ./release.sh patch      # bump 0.1.7 → 0.1.8 then publish
#   ./release.sh minor      # bump 0.1.7 → 0.2.0 then publish
#   ./release.sh --dry-run  # build + package only, skip publish
#   ./release.sh -h|--help  # this message
#
# Requires:
#   - node + npm
#   - VSCE_PAT env var (unless --dry-run). Set non-interactively:
#       read -s VSCE_PAT && export VSCE_PAT
#
# When you're ready to ship per-platform bundled binaries (zero install
# for end users), tag a release on mystiz-miso/cinccino and let the
# `release-extension.yml` workflow do the matrix build.

set -euo pipefail

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
      echo "Unknown arg: $arg (see --help)" >&2
      exit 64
      ;;
  esac
done

cd "$(dirname "$0")"

command -v npm >/dev/null || { echo "ERROR: npm not on PATH" >&2; exit 3; }
if [[ $DRY_RUN -eq 0 && -z "${VSCE_PAT:-}" ]]; then
  echo "ERROR: VSCE_PAT not set. Either --dry-run, or:" >&2
  echo "       read -s VSCE_PAT && export VSCE_PAT" >&2
  exit 3
fi

if [[ -n "$BUMP" ]]; then
  echo "→ bumping version ($BUMP)"
  npm version "$BUMP" --no-git-tag-version >/dev/null
fi
VERSION=$(node -p "require('./package.json').version")
echo "→ extension version: $VERSION"

echo "→ npm install + bundle"
npm install --no-audit --no-fund >/dev/null
npm run compile

# Strip any platform binary staged by an earlier host-mode build so it
# doesn't sneak into the generic .vsix and confuse cross-platform users.
rm -rf server/

VSIX="cinccino-circom-generic.vsix"
rm -f "$VSIX"
echo "→ packaging $VSIX (untargeted)"
npx --yes @vscode/vsce package --no-dependencies --out "$VSIX"

if [[ $DRY_RUN -eq 1 ]]; then
  echo "✓ dry-run complete; .vsix at $(pwd)/$VSIX"
  exit 0
fi

echo "→ publishing $VSIX"
npx --yes @vscode/vsce publish --no-dependencies --packagePath "$VSIX"
echo
echo "✓ published samueltangz.cinccino-circom@$VERSION (untargeted)"
echo "  https://marketplace.visualstudio.com/items?itemName=samueltangz.cinccino-circom"
echo
echo "  Every platform falls through to the extension's auto-cargo-install"
echo "  path on first .circom open. ~2 min one-time per user."
