#! /usr/bin/env bash
set -euxo pipefail

git add -A

nixos-rebuild build --flake .
sudo nixos-rebuild switch --flake .

# nix shell nixpkgs#git --extra-experimental-features nix-command --extra-experimental-features flakes

# useful for debugging the above command
# --option eval-cache false --show-trace
