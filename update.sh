#! /usr/bin/env sh
set -euxo pipefail

git add -A

if test -f /etc/NIXOS; then
    sudo nix flake update
    ./switch.sh
else
    nix flake update
    ./switch.sh
fi
