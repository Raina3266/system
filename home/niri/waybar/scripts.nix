{
  pkgs,
  ...
}:
let
  walker = "${pkgs.walker}/bin/walker";
  notify = "${pkgs.libnotify}/bin/notify-send";
  jq = "${pkgs.jq}/bin/jq";

  todoFile = "\${XDG_CACHE_HOME:-$HOME/.cache}/elephant/todo.csv";
  timerState = "\${XDG_RUNTIME_DIR:-/tmp}/waybar-timer.state";

  # wob FIFO — the wob daemon reads integer percentages from here and
  # renders an overlay progress bar. See systemd.user.services.wob in
  # default.nix.
  wobFifo = "\${XDG_RUNTIME_DIR:-/tmp}/wob.sock";

  # Inlined into each timer script: writes the current remaining
  # percentage to wob. No-op if wob isn't running (no FIFO). Expects
  # shell vars $remaining and $total to be set by the caller.
  wobTick = ''
    if [ -p "${wobFifo}" ] && [ "$total" -gt 0 ] 2>/dev/null; then
      pct=$(( ("$remaining" * 100) / "$total" ))
      [ "$pct" -gt 100 ] && pct=100
      [ "$pct" -lt 0 ] && pct=0
      printf '%d\n' "$pct" > "${wobFifo}" 2>/dev/null &
    fi
  '';
in
{
  # ── Todo list (walker's built-in `todo` provider) ────────────────
  # Shows count of pending tasks on the bar; tooltip lists upcoming
  # tasks ranked by urgency. Left/right-click open walker's todo
  # provider (add/complete/delete/activate).
  todoPoll = pkgs.writeShellScript "waybar-todo-poll" ''
    icon="<span size='x-large'>󰄲</span>"
    if [ ! -f "${todoFile}" ]; then
      printf '{"text":"%s","tooltip":"Todo","class":"clear"}' "$icon"
      exit 0
    fi

    # Count pending (state=pending or urgent) tasks.
    pending=$(awk -F';' 'NR>1 && ($3=="pending" || $3=="urgent") {c++} END{print c+0}' "${todoFile}")
    # Count tasks scheduled for today or overdue.
    actionable=$(awk -F';' 'NR>1 && ($3=="pending" || $3=="urgent") && $6!="" {
      cmd="date -d \"" $6 "\" +%s 2>/dev/null"
      cmd | getline ts; close(cmd)
      cmd="date -d \"today 23:59:59\" +%s"
      cmd | getline eod; close(cmd)
      if (ts!="" && ts+0 <= eod+0) c++
    } END{print c+0}' "${todoFile}")

    if [ "$actionable" -gt 0 ] 2>/dev/null; then
      text="$icon <span size='medium'>$actionable</span>"; class="urgent"
    elif [ "$pending" -gt 0 ] 2>/dev/null; then
      text="$icon <span size='medium'>$pending</span>"; class="pending"
    else
      text="$icon"; class="clear"
    fi

    # Tooltip: up to 10 pending tasks (text + scheduled time).
    list=$(awk -F';' 'NR>1 && ($3=="pending" || $3=="urgent") {
      if ($6!="") {
        print "⬜  " $2 "  (" $6 ")"
      } else {
        print "⬜  " $2
      }
    }' "${todoFile}" | head -10)

    if [ -n "$list" ]; then
      tooltip="$pending pending · $actionable due today/overdue"$'\n\n'"$list"
    else
      tooltip="No pending tasks 🎉"
    fi

    ${jq} -cn --arg text "$text" --arg tooltip "$tooltip" --arg class "$class" \
      '{text:$text, tooltip:$tooltip, class:$class}'
  '';

  # ── Timer module (wob visualizer) ────────────────────────────────
  # State lives in $XDG_RUNTIME_DIR/waybar-timer.state as
  # "<end_epoch> <total_seconds> <label> <paused>". While paused,
  # `end` holds remaining seconds (total still tracks the original).
  #
  # The countdown is visualized by wob (a Wayland overlay bar) via a
  # FIFO at $XDG_RUNTIME_DIR/wob.sock. Each tick writes the remaining
  # percentage. The waybar module is just a launcher icon — see
  # waybar.nix.
  timerPoll = pkgs.writeShellScript "waybar-timer-poll" ''
    state="${timerState}"
    icon="<span size='x-large'>󰔛</span>"
    if [ ! -f "$state" ]; then
      printf '{"text":"%s","tooltip":"Timer"}' "$icon"
      exit 0
    fi
    read -r end total label paused < "$state"
    now=$(date +%s)
    if [ "$paused" = "1" ]; then
      remaining=$end
      text=$(printf " %s <span size='medium'>%d:%02d</span>" "$icon" $((remaining/60)) $((remaining%60)))
      printf '{"text":"%s","tooltip":"Timer paused"}' "$text" "${label:-paused}"
      ${wobTick}
      exit 0
    fi
    remaining=$((end - now))
    if [ "$remaining" -le 0 ]; then
      rm -f "$state"
      ${notify} -u critical "Timer" "⏰ ${label:-Done}"
      # Beep: 880Hz sine wave for 0.3s, played in background so the
      # poll loop isn't blocked. Uses ffplay (ffmpeg) with a lavfi
      # sine source; -nodisp hides the video window, -autoexit quits
      # after the tone finishes.
      (
        ${pkgs.ffmpeg}/bin/ffplay -nodisp -autoexit -f lavfi -i "sine=frequency=880:duration=1" >/dev/null 2>&1
      ) &
      printf '{"text":"%s","tooltip":"Timer"}' "$icon"
      exit 0
    fi
    h=$((remaining / 3600))
    m=$(((remaining % 3600) / 60))
    s=$((remaining % 60))
    if [ "$h" -gt 0 ]; then
      text=$(printf "%s <span size='medium'>%d:%02d:%02d</span>" "$icon" "$h" "$m" "$s")
    else
      text=$(printf "%s <span size='medium'>%d:%02d</span>" "$icon" "$m" "$s")
    fi
    printf '{"text":"%s","tooltip":"Timer running"}' "$text" "${label:-running}"
    ${wobTick}
  '';

  timerSet = pkgs.writeShellScript "waybar-timer-set" ''
    state="${timerState}"
    choice=$(printf '%s\n' \
      "15 min" "20 min" "25 min" "30 min" "45 min" "1 hour" \
      "Custom…" \
      | ${walker} -d -p "Timer" 2>/dev/null)
    [ -z "$choice" ] && exit 0

    case "$choice" in
      *Custom*)
        input=$(${walker} -d -p "Minutes (or e.g. 90s, 2h)" 2>/dev/null)
        [ -z "$input" ] && exit 0
        # If bare number, treat as minutes
        if printf '%s' "$input" | grep -qE '^[0-9]+$'; then
          input="''${input}m"
        fi
        ;;
      *hour*)  input="$(echo "$choice" | awk '{print $1}')h" ;;
      *min*)   input="$(echo "$choice" | awk '{print $1}')m" ;;
      *) exit 0 ;;
    esac

    # Parse durations like 90s, 10m, 2h, or 1h30m
    total=0
    rest="$input"
    while [ -n "$rest" ]; do
      n=$(printf '%s' "$rest" | grep -oE '^[0-9]+')
      [ -z "$n" ] && break
      unit=$(printf '%s' "$rest" | grep -oE '^[0-9]+[a-zA-Z]' | grep -oE '[a-zA-Z]$')
      rest=$(printf '%s' "$rest" | sed -E "s/^$n$unit//")
      case "$unit" in
        s) total=$((total + n)) ;;
        m) total=$((total + n * 60)) ;;
        h) total=$((total + n * 3600)) ;;
        *) break ;;
      esac
    done

    if [ "$total" -le 0 ]; then
      ${notify} "Timer" "Invalid duration: $input"
      exit 0
    fi

    end=$(( $(date +%s) + total ))
    printf '%s %s %s 0\n' "$end" "$total" "$input" > "$state"
    ${notify} "Timer" "Started: $input"
  '';

  timerCancel = pkgs.writeShellScript "waybar-timer-cancel" ''
    state="${timerState}"
    if [ -f "$state" ]; then
      rm -f "$state"
      # Clear the wob bar.
      [ -p "${wobFifo}" ] && printf '0\n' > "${wobFifo}" 2>/dev/null &
      ${notify} "Timer" "Cancelled"
    fi
  '';

  timerTogglePause = pkgs.writeShellScript "waybar-timer-pause" ''
    state="${timerState}"
    if [ ! -f "$state" ]; then exit 0; fi
    read -r end total label paused < "$state"
    if [ "$paused" = "1" ]; then
      # Resume: `end` holds remaining seconds; compute new epoch.
      now=$(date +%s)
      newend=$((now + end))
      printf '%s %s %s 0\n' "$newend" "$total" "$label" > "$state"
      ${notify} "Timer" "Resumed"
    else
      # Pause: store remaining seconds in place of end.
      now=$(date +%s)
      remaining=$((end - now))
      if [ "$remaining" -le 0 ]; then
        rm -f "$state"
        exit 0
      fi
      printf '%s %s %s 1\n' "$remaining" "$total" "$label" > "$state"
      ${notify} "Timer" "Paused"
    fi
  '';

  timerScrollUp = pkgs.writeShellScript "waybar-timer-scroll-up" ''
    state="${timerState}"
    if [ -f "$state" ]; then
      read -r end total label paused < "$state"
      end=$((end + 60))
      printf '%s %s %s %s\n' "$end" "$total" "$label" "$paused" > "$state"
      ${wobTick}
    else
      # No timer running — start a 1-minute timer
      end=$(( $(date +%s) + 60 ))
      printf '%s 60 1m 0\n' "$end" > "$state"
      ${notify} "Timer" "Started: 1m"
      ${wobTick}
    fi
  '';

  timerScrollDown = pkgs.writeShellScript "waybar-timer-scroll-down" ''
    state="${timerState}"
    if [ -f "$state" ]; then
      read -r end total label paused < "$state"
      if [ "$paused" = "1" ]; then
        end=$((end - 60))
        [ "$end" -lt 0 ] && end=0
      else
        end=$((end - 60))
        [ "$end" -lt $(date +%s) ] && end=$(( $(date +%s) + 1 ))
      fi
      printf '%s %s %s %s\n' "$end" "$total" "$label" "$paused" > "$state"
      ${wobTick}
    fi
  '';
}
