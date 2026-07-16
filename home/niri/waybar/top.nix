# Top waybar: clock, tray, hardware/system groups, media, utilities.
# Returns the topBar attrset (without bar outputs — merged by default.nix).
{
  pkgs,
  scripts,
}:
{
  layer = "top";
  position = "top";
  height = 36;
  smooth-scrolling-threshold = 5;

  modules-left = [
    "clock"
    "group/system"
    "group/hardware"
  ];
  modules-center = [
    "group/media"
    "custom/lyrics"
  ];
  modules-right = [
    "tray"
    "custom/cliphist"
    "custom/files"
    "custom/timer"
    "custom/todo"
    "custom/bt"
    "custom/wifi"
    "custom/powermenu"
  ];

  "clock" = {
    # Date + time. Click toggles an alternate full-date format; hover
    # shows the calendar tooltip.
    format = "󰃭 {:%A %d %B  %H:%M}";
    format-alt = "󰃭 {:%A %Y-%m-%d  %H:%M:%S}";
    tooltip-format = "<tt>{calendar}</tt>";
    calendar = {
      mode = "month";
      mode-switcher = true;
      format = {
        months = "<span color='#ff7edb'><b>{}</b></span>";
        weekdays = "<span color='#7afcff'><b>{}</b></span>";
        days = "<span color='#cbe3e7'>{}</span>";
        today = "<span color='#ff3333'><b><u>{}</u></b></span>";
      };
    };
    on-click = "mode_switch";
    on-scroll = "1";
  };

  "group/hardware" = {
    orientation = "horizontal";
    drawer = {
      transition-duration = 300;
      transition-left-to-right = true;
    };
    modules = [
      "temperature"
      "memory"
      "cpu"
      "disk"
      "network"
    ];
  };

  "group/system" = {
    orientation = "horizontal";
    drawer = {
      transition-duration = 300;
      transition-left-to-right = true;
    };
    modules = [
      "custom/battery"
      "backlight"
      "pulseaudio"
    ];
  };

  "power-profiles-daemon" =
    let
      icon = cp: builtins.fromJSON ''"\u${cp}"'';
    in
    {
      format = "{icon}";
      tooltip-format = "Power profile: {profile}\nDriver: {driver}";
      tooltip = true;
      format-icons = {
        default = icon "F0E7"; # nf-fa-bolt
        performance = icon "F135"; # nf-fa-rocket
        balanced = icon "F24E"; # nf-fa-balance_scale
        power-saver = icon "F06C"; # nf-fa-leaf
      };
    };

  "cpu" = {
    format = "󰻠 {usage}%";
    tooltip = true;
    tooltip-format = "CPU: {usage}%\n{avg_frequency} GHz";
  };

  "temperature" = {
    hwmon-path = "";
    thermal-zone = 7;
    critical-threshold = 80;
    interval = 5;
    format = "󰔏 {temperatureC}°C";
    format-critical = "󰔅 {temperatureC}°C";
    tooltip-format = "Sensor: {chip}\n{temperatureC}°C";
  };

  "memory" = {
    interval = 5;
    format = "󰍛 {used:0.1f}G / {total:0.1f}G";
    format-alt = "󰍛 {percentage}%";
    tooltip-format = "RAM: {used:0.1f}G / {total:0.1f}G ({percentage}%)\nSwap: {swapUsed:0.1f}G / {swapTotal:0.1f}G";
  };

  "disk" = {
    format = "󰋊 {free}";
    format-alt = "󰋊 {percentage_used}% ({free})";
    tooltip = true;
  };

  "network" = {
    format = "󰖩  {bandwidthDownBytes}";
    format-disconnected = "󰖪 Disconnected";
    format-alt = "󰖩  {bandwidthUpBytes} |  {bandwidthDownBytes}";
    format-wifi = "󰖩  {bandwidthDownBytes}";
    format-ethernet = "󰈀  {bandwidthDownBytes}";
    tooltip-format-wifi = "󰖩 {essid} ({signalStrength}%)\n {ipaddr}\n {bandwidthUpBytes} /  {bandwidthDownBytes}";
    tooltip-format-ethernet = "󰈀 {ifname}: {ipaddr}/{cidr}\n {bandwidthUpBytes} /  {bandwidthDownBytes}";
    tooltip-format-disconnected = "󰖪 Disconnected";
    on-click-right = "nm-connection-editor";
  };

  "custom/wifi" = {
    format = "{}";
    return-type = "json";
    exec = pkgs.writeShellScript "waybar-wifi-poll" ''
      ssid=$(nmcli -t -f active,ssid dev wifi 2>/dev/null | grep '^yes:' | cut -d: -f2)
      if [ -n "$ssid" ]; then
        signal=$(nmcli -t -f active,signal dev wifi 2>/dev/null | grep '^yes:' | cut -d: -f2)
        printf '{"text":"󰤨","tooltip":"Connected: %s (%s%%)"}' "$ssid" "$signal"
      else
        printf '{"text":"󰤪","tooltip":"Wi-Fi: Disconnected"}'
      fi
    '';
    interval = 5;
    on-click = pkgs.writeShellScript "waybar-wifi" ''
      ${pkgs.walker}/bin/walker -m menus:wifi
    '';
  };

  # Bluetooth — walker's built-in `bluetooth` provider (elephant).
  "custom/bt" = {
    format = "{}";
    return-type = "json";
    exec = pkgs.writeShellScript "waybar-bt-poll" ''
      powered=$(bluetoothctl show 2>/dev/null | grep "Powered:" | awk '{print $2}')
      if [ "$powered" = "yes" ]; then
        names=$(bluetoothctl devices Connected 2>/dev/null | sed 's/^Device [0-9A-Fa-f:]* //' | tr '\n' ',' | sed 's/,$//')
        if [ -n "$names" ]; then
          printf '{"text":"󰂯","tooltip":"Connected: %s"}' "$names"
        else
          printf '{"text":"󰂯","tooltip":"Bluetooth: On (no devices connected)"}'
        fi
      else
        printf '{"text":"󰂱","tooltip":"Bluetooth: Off"}'
      fi
    '';
    interval = 5;
    on-click = pkgs.writeShellScript "waybar-bt" ''
      ${pkgs.walker}/bin/walker -m bluetooth
    '';
  };

  "tray" = {
    icon-size = 18;
    spacing = 10;
  };

  "backlight" = {
    format = "󰃠 {percent}%";
    tooltip-format = "Backlight: {percent}%";
    on-scroll-up = "${pkgs.brightnessctl}/bin/brightnessctl set 5%+";
    on-scroll-down = "${pkgs.brightnessctl}/bin/brightnessctl set 5%-";
    on-click = "${pkgs.brightnessctl}/bin/brightnessctl set 100%";
  };

  "pulseaudio" = {
    format = "󰕾 {volume}%";
    format-bluetooth = "󰕾 {volume}%";
    format-bluetooth-muted = "󰝟 {volume}%";
    format-muted = "󰝟 {volume}%";
    tooltip-format = "Volume: {volume}%";
    scroll-step = 5;
    on-click-right = "pavucontrol";
    on-click = "pactl set-sink-mute 0 toggle";
  };

  "custom/battery" = {
    return-type = "json";
    interval = 5;
    exec = pkgs.writeShellScript "waybar-battery-poll" ''
      bat=$(ls -d /sys/class/power_supply/BAT* 2>/dev/null | head -1)
      if [ -z "$bat" ]; then
        printf '{"text":"","class":"clear"}'
        exit 0
      fi
  
      capacity=$(cat "$bat/capacity" 2>/dev/null)
      status=$(cat "$bat/status" 2>/dev/null)  # Charging / Discharging / Full / Not charging
      profile=$(powerprofilesctl get 2>/dev/null)
  
      icons=("󰁺" "󰁻" "󰁼" "󰁽" "󰁾" "󰁿" "󰂀" "󰂁" "󰂂" "󰁹")
      idx=$(( capacity / 10 ))
      [ "$idx" -gt 9 ] && idx=9
      icon="''${icons[$idx]}"
  
      case "$status" in
        Charging|"Not charging")
          icon="󰂄"
          class="charging"
          ;;
        Full)
          icon="󰂄"
          class="full"
          ;;
        *)
          class="discharging"
          if [ "$capacity" -le 10 ]; then
            class="critical"
          elif [ "$capacity" -le 20 ]; then
            class="warning"
          fi
          ;;
      esac
  
      tooltip="$status | Profile: $profile"
  
      ${pkgs.jq}/bin/jq -cn --arg text "$icon $capacity%" --arg tooltip "$tooltip" --arg class "$class" \
        '{text:$text, tooltip:$tooltip, class:$class}'
    '';
    on-click = pkgs.writeShellScript "waybar-battery-cycle" ''
      current=$(powerprofilesctl get 2>/dev/null)
      case "$current" in
        performance) next="balanced" ;;
        balanced) next="power-saver" ;;
        power-saver) next="performance" ;;
        *) next="balanced" ;;
      esac
      powerprofilesctl set "$next" 2>/dev/null
      notify-send "Power Profile" "Set to $next"
    '';
  };

  # Clipboard history — walker's `clipboard` provider (elephant).
  "custom/cliphist" = {
    format = "󰆏";
    return-type = "json";
    exec = ''echo '{"text":"󰆏","tooltip":"Clipboard history"}' '';
    interval = 86400;
    on-click = pkgs.writeShellScript "waybar-cliphist" ''
      ${pkgs.walker}/bin/walker -m clipboard
    '';
  };

  # Todo count — see scripts.nix for the poll logic.
  "custom/todo" = {
    return-type = "json";
    interval = 2;
    exec = scripts.todoPoll;
    on-click = pkgs.writeShellScript "waybar-todo-open" ''
      ${pkgs.walker}/bin/walker -m todo
    '';
    on-click-right = pkgs.writeShellScript "waybar-todo-add" ''
      ${pkgs.walker}/bin/walker -m todo
    '';
  };

  # Countdown timer — see scripts.nix.
  "custom/timer" = {
    return-type = "json";
    interval = 1;
    exec = scripts.timerPoll;
    on-click = scripts.timerSet;
    on-click-middle = scripts.timerCancel;
    on-click-right = scripts.timerTogglePause;
    on-scroll-up = scripts.timerScrollUp;
    on-scroll-down = scripts.timerScrollDown;
  };

  # Files — elephant `files` provider (fd-backed file search).
  "custom/files" = {
    format = "󰥢";
    return-type = "json";
    exec = ''echo '{"text":"󰥢","tooltip":"Search files"}' '';
    interval = 86400;
    on-click = pkgs.writeShellScript "waybar-files" ''
      ${pkgs.walker}/bin/walker -m files
    '';
  };

  # Media controls group: prev | track info (play/pause) | next.
  # All three hide when no player is running (via exec-if on the prev/next
  # modules, and the stopped CSS class on custom/media). Grouped so they
  # sit together as a single unit in the center.
  "group/media" = {
    orientation = "horizontal";
    modules = [
      "custom/media-prev"
      "custom/media"
      "custom/media-next"
    ];
  };

  # Previous track button. Hidden when no player is running.
  "custom/media-prev" = {
    format = "⏮";
    exec-if = "${pkgs.playerctl}/bin/playerctl -l 2>/dev/null | grep -q .";
    interval = 2;
    on-click = "${pkgs.playerctl}/bin/playerctl previous";
  };

  # Next track button. Hidden when no player is running.
  "custom/media-next" = {
    format = "⏭";
    exec-if = "${pkgs.playerctl}/bin/playerctl -l 2>/dev/null | grep -q .";
    interval = 2;
    on-click = "${pkgs.playerctl}/bin/playerctl next";
  };

  # Media player (center, appears only when playing). Polls playerctl
  # for metadata. Hidden entirely when no player is running via the
  # `stopped` CSS class (display:none). Left-click: play/pause,
  # right-click: next, scroll up/down: prev/next.
  "custom/media" = {
    hide-empty = true;
    format = "{icon} {text}";
    format-icons = {
      "Playing" = "▶";
      "Paused" = "⏸";
      "Stopped" = "⏹";
    };
    return-type = "json";
    exec = pkgs.writeShellScript "waybar-media-poll" ''
      players=$(${pkgs.playerctl}/bin/playerctl -l 2>/dev/null)
      if [ -z "$players" ]; then
          printf '{"text":"","class":"stopped"}'
          exit 0
      fi
      status=$(${pkgs.playerctl}/bin/playerctl status 2>/dev/null)
      [ -z "$status" ] && status="Stopped"
      artist=$(${pkgs.playerctl}/bin/playerctl metadata --format '{{artist}}' 2>/dev/null)
      title=$(${pkgs.playerctl}/bin/playerctl metadata --format '{{title}}' 2>/dev/null)
      player=$(${pkgs.playerctl}/bin/playerctl metadata --format '{{playerName}}' 2>/dev/null)
      title_short=$(printf '%.30s' "$title")
      artist_short=$(printf '%.20s' "$artist")
      if [ -n "$artist_short" ]; then
        text="$artist_short - $title_short"
      else
        text="$title_short"
      fi
      if [ -n "$artist" ]; then
        tooltip="$artist - $title"
      else
        tooltip="$title"
      fi
      [ -n "$player" ] && tooltip="$tooltip\nPlayer: $player\nStatus: $status"
      class=$(echo "$status" | tr '[:upper:]' '[:lower:]')
      ${pkgs.jq}/bin/jq -cn --arg text "$text" --arg class "$class" --arg tooltip "$tooltip" \
        '{text:$text, class:$class, alt:$class, tooltip:$tooltip}'
    '';
    interval = 2;
    on-click = "${pkgs.playerctl}/bin/playerctl play-pause";
  };

  # Lyrics (center). Real-time synced lyrics via waybar-lyric, which
  # fetches from LrcLib / embedded lyrics / .lrc files / MPRIS and emits
  # waybar JSON. Hidden when no player is running (waybar-lyric outputs
  # empty text, and hide-empty-text hides the module).
  # Left-click toggles play/pause via waybar-lyric's subcommand.
  "custom/lyrics" = {
    hide-empty-text = true;
    return-type = "json";
    format = "{icon} {0}";
    format-icons = {
      playing = "▶";
      paused = "⏸";
      lyric = "";
      music = "󰝚";
    };
    exec-if = "which waybar-lyric";
    exec = "${pkgs.waybar-lyric}/bin/waybar-lyric -qfpartial";
    on-click = "${pkgs.waybar-lyric}/bin/waybar-lyric play-pause";
  };

  "custom/powermenu" = {
    format = "󰐥";
    return-type = "json";
    exec = ''echo '{"text":"󰐥","tooltip":"Power menu"}' '';
    interval = 86400;
    # elephant's `menus` provider — defined in walker/default.nix.
    on-click = pkgs.writeShellScript "waybar-powermenu" ''
      ${pkgs.walker}/bin/walker -m menus:power
    '';
  };
}
