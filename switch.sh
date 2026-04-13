#! /usr/bin/env bash
set -euxo pipefail

# ./switch.sh thinkpad
# ./switch.sh dell

NAME="$1"  # either "thinkpad" or "dell"

git add -A

nixos-rebuild build --flake ".#$NAME"
sudo nixos-rebuild switch --flake ".#$NAME"

# nix shell nixpkgs#git --extra-experimental-features nix-command --extra-experimental-features flakes

# useful for debugging the above command
# --option eval-cache false --show-trace
