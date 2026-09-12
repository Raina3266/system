#!/usr/bin/env bash
# Explains why one media source ignores the Wayle media panel.
#
# Read-only by default. --test-control also sends Pause and PlayPause to every
# player and reports what moved, which will pause your music.
#
#   bash mpris-report.sh                > report.txt 2>&1
#   bash mpris-report.sh --test-control > report.txt 2>&1
#
# Run it while the source that misbehaves is PLAYING.

set -uo pipefail

TEST_CONTROL=0
[ "${1:-}" = "--test-control" ] && TEST_CONTROL=1

OBJ=/org/mpris/MediaPlayer2
PLAYER=org.mpris.MediaPlayer2.Player
ROOT=org.mpris.MediaPlayer2

say() { printf '\n===== %s =====\n' "$*"; }

# One line per property, so a player that does not implement one does not
# bury the report in a backtrace.
prop() {
  local out
  if out=$(busctl --user get-property "$1" "$OBJ" "$2" "$3" 2>&1); then
    printf '%s' "$out"
  else
    printf '<unavailable: %s>' "$(printf '%s' "$out" | head -1)"
  fi
}

say "WHEN AND WHICH BUILD"
date -Is
echo "kernel      : $(uname -srm)"
wayle_bin=$(command -v wayle 2>/dev/null)
echo "wayle       : ${wayle_bin:-not on PATH}"
if [ -n "${wayle_bin:-}" ]; then
  echo "wayle store : $(readlink -f "$wayle_bin")"
fi
echo "-- what the media panel reads off the bus --"
media-panel players 2>&1 | head -40
echo "-- repo HEAD --"
git -C "$HOME/system" log --oneline -1 2>&1

say "MPRIS NAMES ON THE SESSION BUS"
mapfile -t PLAYERS < <(
  busctl --user list --acquired --no-legend 2>/dev/null \
    | awk '{print $1}' | grep '^org\.mpris\.MediaPlayer2\.' | sort -u
)
if [ "${#PLAYERS[@]}" -eq 0 ]; then
  echo "none found"
else
  printf '%s\n' "${PLAYERS[@]}"
fi

say "PLAYERCTL'S VIEW"
echo "-- playerctl -l (every player it can see) --"
playerctl -l 2>&1
echo "-- playerctl --all-players status --"
playerctl --all-players status 2>&1
echo "-- playerctl --all-players metadata --"
echo "   (a player missing here is one playerctl refuses to talk to)"
playerctl --all-players metadata 2>&1

for p in "${PLAYERS[@]}"; do
  say "PLAYER $p"

  pid=$(busctl --user call org.freedesktop.DBus /org/freedesktop/DBus \
        org.freedesktop.DBus GetConnectionUnixProcessID s "$p" 2>/dev/null \
        | awk '{print $2}')
  echo "owner pid   : ${pid:-unknown}"
  if [ -n "${pid:-}" ] && [ -r "/proc/$pid/cmdline" ]; then
    echo "owner cmd   : $(tr '\0' ' ' < "/proc/$pid/cmdline" | cut -c1-300)"
  fi

  printf 'Identity    : %s\n' "$(prop "$p" "$ROOT" Identity)"
  printf 'DesktopEntry: %s\n' "$(prop "$p" "$ROOT" DesktopEntry)"
  printf 'Status      : %s\n' "$(prop "$p" "$PLAYER" PlaybackStatus)"
  printf 'Position    : %s\n' "$(prop "$p" "$PLAYER" Position)"
  for cap in CanControl CanPlay CanPause CanSeek CanGoNext CanGoPrevious; do
    printf '%-12s: %s\n' "$cap" "$(prop "$p" "$PLAYER" "$cap")"
  done
  echo "Metadata    :"
  prop "$p" "$PLAYER" Metadata | fold -w 150 | sed 's/^/    /' | head -40
done

say "COVER ART AND FILE PATHS (why a local track shows no picture)"
for p in "${PLAYERS[@]}"; do
  meta=$(prop "$p" "$PLAYER" Metadata)
  echo "-- $p"
  echo "   artUrl : $(printf '%s' "$meta" | grep -o '"mpris:artUrl" s "[^"]*"' | tail -1)"
  echo "   url    : $(printf '%s' "$meta" | grep -o '"xesam:url" s "[^"]*"' | tail -1)"
  echo "   length : $(printf '%s' "$meta" | grep -o '"mpris:length" x [0-9-]*' | tail -1)"
done

say "WAYLE'S OWN VIEW (what the Waybar title badge still reads)"
busctl --user call com.wayle.Media1 /com/wayle/Media com.wayle.Media1 ListPlayers 2>&1
echo "-- active player --"
busctl --user call com.wayle.Media1 /com/wayle/Media com.wayle.Media1 GetActivePlayer 2>&1

say "IS CHROME'S OWN MPRIS DISABLED?"
echo "   (MediaSessionService should appear in --disable-features)"
pgrep -a -f 'chrome|chromium' 2>/dev/null | grep -v -- '--type=' | cut -c1-400

if [ "$TEST_CONTROL" -eq 1 ]; then
  for p in "${PLAYERS[@]}"; do
    say "CONTROL TEST $p"
    before=$(prop "$p" "$PLAYER" PlaybackStatus)
    echo "before      : $before"
    echo "Pause reply : $(busctl --user call "$p" "$OBJ" "$PLAYER" Pause 2>&1 || true)"
    sleep 1
    mid=$(prop "$p" "$PLAYER" PlaybackStatus)
    echo "after Pause : $mid"
    if [ "$before" = "$mid" ]; then
      echo "PlayPause   : $(busctl --user call "$p" "$OBJ" "$PLAYER" PlayPause 2>&1 || true)"
      sleep 1
      echo "after toggle: $(prop "$p" "$PLAYER" PlaybackStatus)"
      echo "VERDICT     : this player ignores both commands"
    else
      echo "VERDICT     : this player obeys Pause"
    fi
  done
fi

say "WAYLE LOG (last 80 lines)"
journalctl --user -u wayle -n 80 --no-pager 2>&1 | tail -80

say "MPRISENCE TODAY"
journalctl --user --since today --no-pager 2>/dev/null | grep -i mprisence | tail -60

say "END"
