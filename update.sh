#!/usr/bin/env bash
# Updates the NixOS flake inputs and the Baidu Netdisk package.
#
# Usage: ./update.sh
set -euo pipefail

cd "$(dirname "$0")"

# --- 1. Update flake inputs ---
echo "=== Updating flake inputs ==="
nix flake update

# --- 2. Update Baidu Netdisk package ---
echo ""
echo "=== Updating Baidu Netdisk ==="
PACKAGE_FILE="home/overlays/packages/baidupan.nix"

# Fetch the latest version info from Baidu's CMS API.
json=$(curl --silent "https://pan.baidu.com/disk/cmsdata?platform=linux&num=1")

# Extract the version number and .deb URL using grep/sed (no jq dependency).
version=$(echo "$json" | grep -oP '"version":"[^"]*V\K[0-9.]+' | head -1)
url=$(echo "$json" | grep -oP '"url_1":"\K[^"]+' | head -1)

if [ -z "$version" ] || [ -z "$url" ]; then
  echo "Failed to parse version or URL from Baidu API response" >&2
  echo "API response: $json" >&2
  exit 1
fi

echo "Latest version: $version"
echo "Download URL:   $url"

# Update the version line.
sed -i "s|version = \"[^\"]*\";|version = \"$version\";|" "$PACKAGE_FILE"

# Update the debUrl line.
sed -i "s|debUrl = \"[^\"]*\";|debUrl = \"$url\";|" "$PACKAGE_FILE"

# Re-fetch the hash. nix-prefetch-url downloads the file and computes the hash.
hash=$(nix-prefetch-url --type sha256 "$url" 2>/dev/null)
sri_hash=$(nix hash to-sri --type sha256 "$hash" 2>/dev/null)

if [ -n "$sri_hash" ]; then
  sed -i "s|hash = \"[^\"]*\";|hash = \"$sri_hash\";|" "$PACKAGE_FILE"
  echo "Updated hash: $sri_hash"
else
  echo "WARNING: Could not compute hash. Update it manually." >&2
fi

echo ""
echo "Done. Updated $PACKAGE_FILE to version $version."
echo "Run 'nixos-rebuild build --flake .#raina' to verify."
