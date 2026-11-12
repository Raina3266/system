#!/usr/bin/env bash
set -euxo pipefail

nix flake update
npm update --prefix scripts/node-types --package-lock-only --ignore-scripts --no-audit --no-fund
