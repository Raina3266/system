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

  # Static launcher icons
  staticLauncher =
    name: icon: tooltip: walkerArgs:
    {
      format = "<span size='large'>${icon}</span>";
      return-type = "json";
      exec = pkgs.writeShellScript "waybar-${name}-poll" ''
        printf '{"text":"<span size='"'"'large'"'"'>${icon}</span>","tooltip":"${tooltip}"}'
      '';
      interval = 86400;
      on-click = pkgs.writeShellScript "waybar-${name}" ''
        ${walker} ${walkerArgs}
      '';
    };

  # Timer with wob visualization
  # State: "<end_epoch> <total_seconds> <label> <paused>"
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
  "custom/audio-sink" = {
    format = "<span size='large'>󰕾</span>";
    return-type = "json";
    interval = 5;
    exec = pkgs.writeShellScript "waybar-audio-sink-poll" ''
      icon="<span size='large'>󰕾</span>"
      default_sink=$(${pkgs.wireplumber}/bin/wpctl status 2>/dev/null | grep -A1 "Audio" | grep -oP 'id: \K[^,]+' | head -1)
      if [ -z "$default_sink" ]; then
        printf '{"text":"%s","tooltip":"Audio output"}' "$icon"
        exit 0
      fi
      description=$(${pkgs.wireplumber}/bin/wpctl status 2>/dev/null | grep -A5 "$default_sink" | grep -oP 'description: "\K[^"]+' | head -1)
      if [ -z "$description" ]; then
        description="$default_sink"
      fi
      printf '{"text":"%s","tooltip":"Audio: %s"}' "$icon" "$description"
    '';
    on-click = pkgs.writeShellScript "waybar-audio-sink-switch" ''
      ${walker} -m menus:audio-sink
    '';
  };

  "tray" = {
    icon-size = 18;
    spacing = 10;
  };

  "custom/cliphist" = staticLauncher "cliphist" "󰕛" "Clipboard history" "-m clipboard";

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

  "custom/bt" = {
    format = "{}";
    return-type = "json";
    exec = pkgs.writeShellScript "waybar-bt-poll" ''
      on_icon="<span size='large'>󰂯</span>"
      off_icon="<span size='large'>󰂱</span>"
      powered=$(bluetoothctl show 2>/dev/null | grep "Powered:" | awk '{print $2}')
      if [ "$powered" = "yes" ]; then
        names=$(bluetoothctl devices Connected 2>/dev/null | sed 's/^Device [0-9A-Fa-f:]* //' | tr '\n' ',' | sed 's/,$//')
        if [ -n "$names" ]; then
          printf '{"text":"%s","tooltip":"Connected: %s"}' "$on_icon" "$names"
        else
          printf '{"text":"%s","tooltip":"Bluetooth: On (no devices connected)"}' "$on_icon"
        fi
      else
        printf '{"text":"%s","tooltip":"Bluetooth: Off"}' "$off_icon"
      fi
    '';
    interval = 5;
    # Left-click: open walker's bluetooth provider to pair/connect/etc.
    # Right-click: toggle power directly via bluetoothctl. This bypasses a
    # walker bug where the "Power On" keybind-hint button never fires when
    # bluetooth is off -- elephant's bluetooth provider returns an empty
    # device list while off, so walker has nothing selected and its click
    # handler (which only activates on a *selected* list item) never runs.
    on-click = pkgs.writeShellScript "waybar-bt" ''
      ${walker} -m bluetooth
    '';
    on-click-right = pkgs.writeShellScript "waybar-bt-toggle-power" ''
      powered=$(bluetoothctl show 2>/dev/null | grep "Powered:" | awk '{print $2}')
      if [ "$powered" = "yes" ]; then
        bluetoothctl power off
        notify-send "Bluetooth" "Powered off"
      else
        bluetoothctl power on
        notify-send "Bluetooth" "Powered on"
      fi
    '';
  };

  "custom/powermenu" = staticLauncher "powermenu" "󰐥" "Power menu" "-m menus:power";
}
