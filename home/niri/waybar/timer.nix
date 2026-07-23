# Timer module for waybar, with wob overlay visualization.
#
# State file: "$XDG_RUNTIME_DIR/waybar-timer.state" containing
#   "<end_epoch> <total_seconds> <label> <paused>"
# When paused, the end_epoch field stores the remaining seconds instead.
#
# Click: set duration (walker dmenu)  |  Middle-click: cancel
# Right-click: pause/resume           |  Scroll: +/- 1 minute
{ pkgs }:
let
  walker = "${pkgs.walker}/bin/walker";
  notify = "${pkgs.libnotify}/bin/notify-send";

  timerState = "\${XDG_RUNTIME_DIR:-/tmp}/waybar-timer.state";

  # wob progress bar FIFO
  wobFifo = "\${XDG_RUNTIME_DIR:-/tmp}/wob.sock";

  # Update wob progress bar (expects $remaining and $total vars)
  wobTick = ''
    if [ -p "${wobFifo}" ] && [ "$total" -gt 0 ] 2>/dev/null; then
      pct=$(( ("$remaining" * 100) / "$total" ))
      [ "$pct" -gt 100 ] && pct=100
      [ "$pct" -lt 0 ] && pct=0
      printf '%d\n' "$pct" > "${wobFifo}" 2>/dev/null &
    fi
  '';

  timerPoll = pkgs.writeShellScript "waybar-timer-poll" ''
    state="${timerState}"
    icon="<span size='large'>󰔛</span>"
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
      # Play completion beep
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
        # Bare number = minutes
        if printf '%s' "$input" | grep -qE '^[0-9]+$'; then
          input="''${input}m"
        fi
        ;;
      *hour*)  input="$(echo "$choice" | awk '{print $1}')h" ;;
      *min*)   input="$(echo "$choice" | awk '{print $1}')m" ;;
      *) exit 0 ;;
    esac

    # Parse duration (90s, 10m, 2h, 1h30m)
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
      # Clear wob
      [ -p "${wobFifo}" ] && printf '0\n' > "${wobFifo}" 2>/dev/null &
      ${notify} "Timer" "Cancelled"
    fi
  '';

  timerTogglePause = pkgs.writeShellScript "waybar-timer-pause" ''
    state="${timerState}"
    if [ ! -f "$state" ]; then exit 0; fi
    read -r end total label paused < "$state"
    if [ "$paused" = "1" ]; then
      # Resume: convert remaining to new epoch
      now=$(date +%s)
      newend=$((now + end))
      printf '%s %s %s 0\n' "$newend" "$total" "$label" > "$state"
      ${notify} "Timer" "Resumed"
    else
      # Pause: store remaining in end field
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
      # No timer - start 1m
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
      end=$((end - 60))
      if [ "$paused" = "1" ]; then
        [ "$end" -lt 0 ] && end=0
      else
        [ "$end" -lt $(date +%s) ] && end=$(( $(date +%s) + 1 ))
      fi
      printf '%s %s %s %s\n' "$end" "$total" "$label" "$paused" > "$state"
      ${wobTick}
    fi
  '';
in
{
  "custom/timer" = {
    return-type = "json";
    interval = 1;
    exec = timerPoll;
    on-click = timerSet;
    on-click-middle = timerCancel;
    on-click-right = timerTogglePause;
    on-scroll-up = timerScrollUp;
    on-scroll-down = timerScrollDown;
  };
}
