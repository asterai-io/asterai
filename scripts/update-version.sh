#!/usr/bin/env bash
# Updates version across all Cargo.toml and npm package.json files.
set -euo pipefail

if [ $# -ne 1 ]; then
  echo "Usage: $0 <version>"
  echo "Example: $0 1.0.1"
  exit 1
fi

VERSION="$1"

# Validate version format (basic semver check).
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$ ]]; then
  echo "Error: Invalid version format '$VERSION'"
  echo "Expected format: X.Y.Z or X.Y.Z-prerelease"
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"

echo "Updating version to $VERSION..."

# Update Cargo.toml files.
echo "  cli/Cargo.toml"
sed -i "s/^version = \".*\"/version = \"$VERSION\"/" "$ROOT_DIR/cli/Cargo.toml"

echo "  asterai/Cargo.toml"
sed -i "s/^version = \".*\"/version = \"$VERSION\"/" "$ROOT_DIR/asterai/Cargo.toml"

# Update npm package.json files.
for pkg in "$ROOT_DIR"/npm/*/package.json; do
  rel_path="${pkg#$ROOT_DIR/}"
  echo "  $rel_path"
  sed -i "s/\"version\": \".*\"/\"version\": \"$VERSION\"/" "$pkg"
done

# Update optionalDependencies in main package.
echo "  npm/asterai/package.json (optionalDependencies)"
sed -i "s/\"@asterai-io\/cli-linux-x64\": \".*\"/\"@asterai-io\/cli-linux-x64\": \"$VERSION\"/" "$ROOT_DIR/npm/asterai/package.json"
sed -i "s/\"@asterai-io\/cli-linux-arm64\": \".*\"/\"@asterai-io\/cli-linux-arm64\": \"$VERSION\"/" "$ROOT_DIR/npm/asterai/package.json"
sed -i "s/\"@asterai-io\/cli-darwin-x64\": \".*\"/\"@asterai-io\/cli-darwin-x64\": \"$VERSION\"/" "$ROOT_DIR/npm/asterai/package.json"
sed -i "s/\"@asterai-io\/cli-darwin-arm64\": \".*\"/\"@asterai-io\/cli-darwin-arm64\": \"$VERSION\"/" "$ROOT_DIR/npm/asterai/package.json"
sed -i "s/\"@asterai-io\/cli-win32-x64\": \".*\"/\"@asterai-io\/cli-win32-x64\": \"$VERSION\"/" "$ROOT_DIR/npm/asterai/package.json"

echo ""
echo "Done. Updated to version $VERSION"
echo ""
echo "Next steps:"
echo "  1. Review changes: git diff"
echo "  2. Commit: git commit -am \"chore: bump version to $VERSION\""
echo "  3. Tag: git tag $VERSION"
echo "  4. Push: git push && git push --tags"
echo "  5. Create GitHub Release for tag $VERSION"
