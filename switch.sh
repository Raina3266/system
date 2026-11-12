#! /usr/bin/env bash
set -euxo pipefail

# Refresh this upstream integration on every switch, not only on full updates.
nix flake update dolphin-gdrive-actions

git add -A

# `switch` builds before it activates, and refuses to activate a failed build,
# so the separate `nixos-rebuild build` that used to run here only bought a
# second full evaluation of the flake.
sudo nixos-rebuild switch --flake .

# nix shell nixpkgs#git --extra-experimental-features nix-command --extra-experimental-features flakes

# useful for debugging the above command
# --option eval-cache false --show-trace
