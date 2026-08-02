#!/usr/bin/env bash
# Instant file picker backed by plocate's index (covers all of $HOME,
# including the rclone GoogleDrive mount, without scanning it live).
# The index is built hourly by the updatedb-home systemd user timer;
# run `systemctl --user start updatedb-home` to refresh it manually.
set -euo pipefail

db="$HOME/.local/share/plocate/home.db"

if [[ ! -s $db ]]; then
  notify-send "rofi-files" "Index not built yet — run: systemctl --user start updatedb-home" 2>/dev/null || true
  exit 1
fi

choice=$(
  plocate -d "$db" "" \
    | grep -v -E '/(\..*|node_modules|build|dist|target|out|result|bin|__pycache__|venv|\.venv|\.cache|\.mypy_cache|\.pytest_cache|\.gradle|\.cargo|\.git|\.svn|\.hg|\.idea|\.vscode|tmp|\.tmp)(/|$)' \
    | sed "s|^${HOME}|~|" \
    | rofi -dmenu -i -matching fuzzy -p "  " -theme-str 'window {width: 50%;}'
)

[[ -n $choice ]] && xdg-open "$HOME/${choice#\~/}"
