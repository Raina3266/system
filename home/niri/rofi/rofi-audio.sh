#!/bin/sh
# Rofi script-mode audio device picker — replaces the former Walker "menus:audio"
# Lua provider. Faithful port of walker/audio.nix: list WirePlumber sinks or
# sources, mark the default, show a volume bar, and support volume +/-, mute and
# outputs/inputs toggle.
#
# Hotkeys map to ROFI_RETV (set by the -kb-custom-N args in the waybar launcher):
#   Return (1)   set default output/input   (closes the menu, like walker)
#   kb-1   (10)  volume +5%                 (rofi reloads the script)
#   kb-2   (11)  volume -5%
#   kb-3   (12)  toggle mute
#   kb-4   (13)  toggle outputs | inputs
# The outputs/inputs view persists across reloads via ROFI_DATA.

export PATH="@wpctlPath@:$PATH"
export LC_ALL=C

WPCTL=wpctl
STEP="5%"

retv=${ROFI_RETV:-0}
info=${ROFI_INFO:-}
mode=${ROFI_DATA:-outputs}
case "$mode" in
  inputs*) mode=inputs ;;
  *) mode=outputs ;;
esac

# Act on the currently selected device (its id is carried in ROFI_INFO).
case "$retv" in
  1)  [ -n "$info" ] && "$WPCTL" set-default "$info" ;;
  10) [ -n "$info" ] && "$WPCTL" set-volume -l 1.0 "$info" "${STEP}+" ;;
  11) [ -n "$info" ] && "$WPCTL" set-volume "$info" "${STEP}-" ;;
  12) [ -n "$info" ] && "$WPCTL" set-mute "$info" toggle ;;
  13) if [ "$mode" = inputs ]; then mode=outputs; else mode=inputs; fi ;;
esac

if [ "$mode" = inputs ]; then
  first=Sources; last=Filters;  kind=input;  prompt=Inputs
else
  first=Sinks;   last=Sources; kind=output; prompt=Outputs
fi

# Rofi script-mode header line (NUL-led). Persist the view via `data` so it
# round-trips through ROFI_DATA on the next invocation.
printf '\000prompt\037%s\000message\037Return: set default   Ctrl+Up/Down: volume   Ctrl+M: mute   Ctrl+T: inputs/outputs\000data\037%s\000no-custom\037true\000keep-selection\037true\n' \
  "$prompt" "$mode"

# Parse the relevant block of `wpctl status` (first Audio tree only, so webcams
# in the Video block never surface as inputs) and emit one rofi row per device.
# Row format: <text>\0info\037<id>\n  (NUL then the `info` option, newline ends
# the entry). gawk with LC_ALL=C prints raw bytes for %c (NUL / unit-separator).
"$WPCTL" status 2>/dev/null |
  sed -n "0,/^${first}:/d; /^${last}:/q; p" |
  awk -v kind="$kind" '
    BEGIN { printed = 0 }
    {
      if (!match($0, /[0-9]+\./)) next
      id = substr($0, RSTART, RLENGTH-1)
      # The default marker "*" only appears before the id on the line.
      pre = substr($0, 1, RSTART-1)
      isdef = (index(pre, "*") > 0)
      rest = substr($0, RSTART+RLENGTH); sub(/^[ \t]+/, "", rest)
      if (!match(rest, /[ \t]*\[vol:/)) next
      desc = substr(rest, 1, RSTART-1); sub(/[ \t]+$/, "", desc)
      after = substr(rest, RSTART+RLENGTH)
      if (!match(after, /^[0-9]+\.?[0-9]*/)) next
      vol = int((substr(after, 1, RLENGTH) + 0) * 100 + 0.5)
      muted = (index($0, "MUTED") > 0)
      filled = int((vol + 9) / 20); if (filled > 5) filled = 5
      bar = ""
      for (i = 0; i < filled; i++) bar = bar "█"
      for (i = filled; i < 5;   i++) bar = bar "·"
      if (muted) sub = "muted · " vol "%"; else sub = bar "  " vol "%"
      marker = isdef ? "✓" : " "
      printf "%s  %s   %s", marker, desc, sub
      printf "%c", 0
      printf "info%c%s", 31, id
      printf "\n"
      printed = 1
    }
    END {
      if (!printed) printf "No audio %ss found%cnonselectable%ctrue\n", kind, 0, 31
    }
  '