{
  pkgs,
  lib,
  config,
  osConfig,
  ...
}: let
  cfg = config.programs'.waybar;

  # Wrap `cliphist store` so that every stored entry also gets a
  # timestamp recorded in a sidecar file. The prune service below
  # uses this to evict entries older than 3 days (cliphist itself
  # only supports a count-based `max-items` limit, not age-based).
  cliphist-store-timed = pkgs.writeShellScriptBin "cliphist-store-timed" ''
    ts_dir="''${XDG_CACHE_HOME:-$HOME/.cache}/cliphist"
    mkdir -p "$ts_dir"
    ts_file="$ts_dir/timestamps"
    now=$(date +%s)
    # Read stdin into a variable so we can pass it to cliphist and
    # also compute the id it will assign (the current max id + 1).
    input=$(cat)
    # Store via cliphist, then record the id it assigned. cliphist
    # lists entries newest-first as "<id>\t<preview>"; the newest
    # id is on the first line.
    printf '%s' "$input" | ${pkgs.cliphist}/bin/cliphist store
    newest_id=$(${pkgs.cliphist}/bin/cliphist list 2>/dev/null | head -1 | cut -f1)
    [ -n "$newest_id" ] && printf '%s\t%s\n' "$newest_id" "$now" >> "$ts_file"
  '';

  # Prune cliphist entries older than 3 days, using the sidecar
  # timestamp file maintained by cliphist-store-timed above.
  cliphist-prune = pkgs.writeShellScriptBin "cliphist-prune" ''
    ts_file="''${XDG_CACHE_HOME:-$HOME/.cache}/cliphist/timestamps"
    [ -f "$ts_file" ] || exit 0
    cutoff=$(($(date +%s) - 3 * 24 * 60 * 60))
    to_delete=""
    while IFS=$'\t' read -r id ts; do
      [ -z "$id" ] || [ -z "$ts" ] && continue
      if [ "$ts" -lt "$cutoff" ] 2>/dev/null; then
        to_delete+="$id"$'\n'
      fi
    done < "$ts_file"
    if [ -n "$to_delete" ]; then
      printf '%s' "$to_delete" | ${pkgs.cliphist}/bin/cliphist delete 2>/dev/null || true
    fi
    # Rebuild the timestamp file to drop ids no longer in the db.
    tmp=$(mktemp)
    ${pkgs.cliphist}/bin/cliphist list 2>/dev/null | cut -f1 | while read -r id; do
      grep -P "^$id\t" "$ts_file" >> "$tmp" 2>/dev/null || true
    done
    mv "$tmp" "$ts_file" 2>/dev/null || rm -f "$tmp"
  '';
in
  with lib; {
    options.programs'.waybar = {
      enable = mkEnableOption "waybar";
      enableHyprlandIntegration = mkEnableOption "Hyprland workspace switcher";
      enableNiriIntegration = mkEnableOption "Niri workspace switcher";
      enableLyrics = mkEnableOption "waybar-lyric in the center";
    };

    config = mkIf (pkgs.stdenv.isLinux && cfg.enable) (mkMerge [
      {
        home.packages = with pkgs; [
          cliphist
          wl-clipboard
          jq
        ] ++ (optional cfg.enableLyrics pkgs.waybar-lyric);

        systemd.user.services.cliphist = {
          Unit = {
            Description = "cliphist Wayland clipboard history daemon";
            PartOf = [ "graphical-session.target" ];
            After = [ "graphical-session.target" ];
          };
          Service = {
            ExecStart = "${pkgs.wl-clipboard}/bin/wl-paste --watch ${cliphist-store-timed}/bin/cliphist-store-timed";
            Restart = "on-failure";
            RestartSec = 3;
          };
          Install = {
            WantedBy = [ "graphical-session.target" ];
          };
        };

        # Prune clipboard history older than 3 days, every hour.
        systemd.user.services.cliphist-prune = {
          Unit = {
            Description = "Prune cliphist entries older than 3 days";
            PartOf = [ "graphical-session.target" ];
            After = [ "graphical-session.target" ];
          };
          Service = {
            Type = "oneshot";
            ExecStart = "${cliphist-prune}/bin/cliphist-prune";
          };
        };
        systemd.user.timers.cliphist-prune = {
          Unit = {
            Description = "Hourly cliphist pruning";
          };
          Timer = {
            OnBootSec = "5min";
            OnUnitActiveSec = "1h";
          };
          Install = {
            WantedBy = [ "timers.target" ];
          };
        };

        systemd.user.services.waybar = {
          Unit = {
            # Only run under niri — GNOME/Mutter lacks layer-shell support
            # and waybar would crash-loop there.
            ConditionEnvironment = lib.mkForce ["XDG_CURRENT_DESKTOP=niri"];
          };
          Service = {
            Restart = lib.mkForce "on-failure";
            RestartSec = 3;
          };
        };
      }

      (mkIf cfg.enableHyprlandIntegration {
        wayland.windowManager.hyprland.settings = {
          exec-once = [
            "waybar"
          ];

          bind = [
            "SUPER, b, exec, ${pkgs.killall}/bin/killall -SIGUSR1 .waybar-wrapped"
          ];
        };
      })

      (lib.mkIf (osConfig != null) {
        programs.waybar = {
          enable = true;
          systemd = {
            enable = true;
          };
          style = ./themes/waybar-cyberpunk.css;

          settings = {
            topBar = ({
              layer = "top";
              position = "top";
              height = 36;
              smooth-scrolling-threshold = 5;

              modules-left = ["clock" "group/hardware"];
              modules-right = ["custom/cliphist" "custom/timer" "tray" "custom/bt" "custom/wifi" "group/system" "custom/powermenu"];
              modules-center =
                []
                ++ (optional cfg.enableLyrics "custom/lyrics");

              "clock" = {
                # Date + ISO week number + time. Click toggles an alternate
                # full-date format; hover shows the calendar tooltip.
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
                  transition-left-to-right = false;
                };
                modules = ["temperature" "memory" "cpu" "disk" "network"];
              };

              "group/system" = {
                orientation = "horizontal";
                drawer = {
                  transition-duration = 300;
                  transition-left-to-right = false;
                };
                modules = ["battery" "backlight" "pulseaudio"];
              };

              "power-profiles-daemon" = let
                icon = cp: builtins.fromJSON ''"\u${cp}"'';
              in {
                format = "{icon}";
                tooltip-format = "Power profile: {profile}\nDriver: {driver}";
                tooltip = true;
                format-icons = {
                  default = icon "F0E7";      # nf-fa-bolt
                  performance = icon "F135";  # nf-fa-rocket
                  balanced = icon "F24E";     # nf-fa-balance_scale
                  power-saver = icon "F06C";  # nf-fa-leaf
                };
              };

              "cpu" = {
                format = "󰻠 {usage}%";
                tooltip = true;
                tooltip-format = "CPU: {usage}%\n{avg_frequency} GHz";
              };

              "temperature" = {
                hwmon-path = "";
                thermal-zone = 0;
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
                format = "󰤨";
                return-type = "json";
                exec = ''echo '{"text":"󰤨","tooltip":"Wi-Fi networks\nLeft-click to scan & connect"}'  '';
                interval = 86400;
                on-click = pkgs.writeShellScript "waybar-wifi" ''
                  # Get current connection for highlighting
                  current=$(nmcli -t -f active,ssid dev wifi 2>/dev/null | grep '^yes:' | cut -d: -f2)

                  # Build list of available networks (rescan first)
                  nmcli device wifi rescan 2>/dev/null
                  mapfile -t lines < <(nmcli -t -f ssid,signal,security dev wifi 2>/dev/null | sort -t: -k2 -nr | awk -F: '!seen[$1]++' | head -20)

                  if [ ''${#lines[@]} -eq 0 ]; then
                    notify-send "Wi-Fi" "No networks found."
                    exit 0
                  fi

                  # Format for rofi: signal bars + ssid + security
                  choices=""
                  for line in "''${lines[@]}"; do
                    IFS=':' read -r ssid signal security <<< "$line"
                    [ -z "$ssid" ] && continue
                    if [ "$ssid" = "$current" ]; then
                      marker="*"
                    else
                      marker=" "
                    fi
                    # Signal bars
                    if [ "$signal" -ge 80 ]; then bars="████"
                    elif [ "$signal" -ge 60 ]; then bars="███ "
                    elif [ "$signal" -ge 40 ]; then bars="██  "
                    elif [ "$signal" -ge 20 ]; then bars="█   "
                    else bars="    "
                    fi
                    sec=""
                    [ -n "$security" ] && sec=" [$security]"
                    choices+="''${marker} ''${bars} ''${ssid}''${sec}\n"
                  done

                  sel=$(echo -e "$choices" | rofi -dmenu -p "Wi-Fi" -i -no-custom 2>/dev/null)
                  [ -z "$sel" ] && exit 0

                  # Extract ssid (strip marker, bars, security)
                  ssid=$(echo "$sel" | sed -E 's/^[* ]+ [█ ]+ //; s/ \[.*\]$//')
                  [ -z "$ssid" ] && exit 0

                  # Check if already connected
                  if [ "$ssid" = "$current" ]; then
                    notify-send "Wi-Fi" "Already connected to $ssid"
                    exit 0
                  fi

                  # Try to connect (works for known networks without prompt)
                  notify-send "Wi-Fi" "Connecting to $ssid..."
                  if nmcli device wifi connect "$ssid" 2>/dev/null; then
                    notify-send "Wi-Fi" "Connected to $ssid"
                  else
                    # Unknown network — open nm-connection-editor for password entry
                    nm-connection-editor &
                  fi
                '';
              };

              "custom/bt" = {
                format = "󰂯";
                return-type = "json";
                exec = ''echo '{"text":"󰂯","tooltip":"Bluetooth devices\nLeft-click to scan & connect"}'  '';
                interval = 86400;
                on-click = pkgs.writeShellScript "waybar-bt" ''
                  if ! command -v bluetoothctl >/dev/null 2>&1; then
                    notify-send "Bluetooth" "bluetoothctl not found"
                    exit 0
                  fi

                  # Make sure bluetooth is on and scan for a few seconds
                  bluetoothctl power on 2>/dev/null
                  bluetoothctl scan on 2>/dev/null &
                  scan_pid=$!
                  sleep 4
                  kill $scan_pid 2>/dev/null
                  bluetoothctl scan off 2>/dev/null

                  # Get paired devices
                  mapfile -t paired < <(bluetoothctl devices Paired 2>/dev/null | sed -E 's/^Device ([0-9A-F:]+) (.+)$/\1\t\2/')

                  # Get discovered (non-paired) devices
                  mapfile -t discovered < <(bluetoothctl devices 2>/dev/null | sed -E 's/^Device ([0-9A-F:]+) (.+)$/\1\t\2/' | while IFS=$'\t' read -r mac name; do
                    if ! printf '%s\n' "''${paired[@]}" | grep -q "^$mac"; then
                      printf '%s\t%s\n' "$mac" "$name"
                    fi
                  done)

                  if [ ''${#paired[@]} -eq 0 ] && [ ''${#discovered[@]} -eq 0 ]; then
                    notify-send "Bluetooth" "No devices found."
                    exit 0
                  fi

                  # Build rofi list
                  choices=""
                  # Paired devices first
                  for entry in "''${paired[@]}"; do
                    IFS=$'\t' read -r mac name <<< "$entry"
                    [ -z "$mac" ] && continue
                    connected=$(bluetoothctl info "$mac" 2>/dev/null | grep -q 'Connected: yes' && echo '✓' || echo ' ')
                    choices+="$connected  $name\n"
                  done
                  # Separator
                  [ ''${#paired[@]} -gt 0 ] && [ ''${#discovered[@]} -gt 0 ] && choices+="─────────────\n"
                  # Discovered devices
                  for entry in "''${discovered[@]}"; do
                    IFS=$'\t' read -r mac name <<< "$entry"
                    [ -z "$mac" ] && continue
                    choices+=" +  $name\n"
                  done

                  sel=$(echo -e "$choices" | rofi -dmenu -p "Bluetooth" -i -no-custom 2>/dev/null)
                  [ -z "$sel" ] && exit 0

                  # Skip separator
                  [[ "$sel" == *─* ]] && exit 0

                  # Parse selection
                  action=$(echo "$sel" | cut -c1)
                  name=$(echo "$sel" | sed -E 's/^[✓ + ]+  //')
                  [ -z "$name" ] && exit 0

                  # Find MAC by name
                  mac=$(bluetoothctl devices 2>/dev/null | grep -i "$name" | head -1 | awk '{print $2}')
                  [ -z "$mac" ] && exit 0

                  case "$action" in
                    ✓)
                      # Connected — disconnect
                      notify-send "Bluetooth" "Disconnecting $name..."
                      bluetoothctl disconnect "$mac" 2>/dev/null
                      ;;
                    ' ')
                      # Paired but not connected — connect
                      notify-send "Bluetooth" "Connecting to $name..."
                      bluetoothctl connect "$mac" 2>/dev/null && notify-send "Bluetooth" "Connected to $name" || notify-send "Bluetooth" "Failed to connect to $name"
                      ;;
                    +)
                      # Not paired — pair, trust, connect
                      notify-send "Bluetooth" "Pairing $name..."
                      bluetoothctl pair "$mac" 2>/dev/null && \
                      bluetoothctl trust "$mac" 2>/dev/null && \
                      bluetoothctl connect "$mac" 2>/dev/null && \
                      notify-send "Bluetooth" "Paired & connected to $name" || notify-send "Bluetooth" "Failed to pair $name"
                      ;;
                  esac
                '';
              };

              "tray" = {
                icon-size = 18;
                spacing = 10;
              };

              "backlight" = {
                format = "󰃠 {percent}%";
                on-scroll-up = "${pkgs.brightnessctl}/bin/brightnessctl set 5%+";
                on-scroll-down = "${pkgs.brightnessctl}/bin/brightnessctl set 5%-";
                on-click = "${pkgs.brightnessctl}/bin/brightnessctl set 100%";
              };

              "pulseaudio" = {
                format = "󰕾 {volume}%";
                format-bluetooth = "󰕾  {volume}%";
                format-bluetooth-muted = "󰝟 {volume}%";
                format-muted = "󰝟 {volume}%";
                tooltip-format = "󰕾 {desc} // {volume}%";
                scroll-step = 5;
                on-click-right = "pavucontrol";
                on-click = "pactl set-sink-mute 0 toggle";
              };

              "battery" = {
                format = "{icon} {capacity}%";
                format-charging = "󰂄 {capacity}%";
                format-icons = ["󰁺" "󰁻" "󰁼" "󰁽" "󰁾" "󰁿" "󰂀" "󰂁" "󰂂" "󰁹"];
                format-plugged = "󰂄 {capacity}%";
                states = {
                  warning = 20;
                  critical = 10;
                };
                interval = 5;
                on-click = pkgs.writeShellScript "waybar-battery-profile" ''
                  current=$(powerprofilesctl get 2>/dev/null | head -1 | awk '{print $3}')
                  choice=$(rofi -dmenu -p "Power Profile" -no-custom -i -selected-row 0 <<EOF
                  performance
                  balanced
                  power-saver
                  EOF
                  )
                  [ -z "$choice" ] && exit 0
                  powerprofilesctl set "$choice" 2>/dev/null
                  notify-send "Power Profile" "Set to $choice"
                '';
              };

              "custom/lyrics" = {
                return-type = "json";
                format = "{icon} {0}";
                hide-empty-text = true;
                format-icons = {
                  playing = "▶";
                  paused = "⏸";
                  lyric = "🎵";
                  music = "🎵";
                };
                exec-if = "which waybar-lyric";
                exec = "waybar-lyric -qfpartial";
                on-click = "waybar-lyric play-pause";
              };

              "custom/cliphist" = {
                format = "󰆏";
                return-type = "json";
                exec = ''echo '{"text":"󰆏","tooltip":"Clipboard history\nLeft-click to browse\nRight-click to clear"}'  '';
                interval = 86400;
                on-click = pkgs.writeShellScript "waybar-cliphist" ''
                  ${pkgs.cliphist}/bin/cliphist list \
                    | rofi -dmenu -p "Clipboard" -i \
                    | ${pkgs.cliphist}/bin/cliphist decode \
                    | ${pkgs.wl-clipboard}/bin/wl-copy
                '';
                on-click-right = pkgs.writeShellScript "waybar-cliphist-clear" ''
                  ${pkgs.cliphist}/bin/cliphist wipe
                  ${pkgs.libnotify}/bin/notify-send "Clipboard" "History cleared"
                '';
              };

              "custom/timer" = {
                return-type = "json";
                interval = 1;
                exec = pkgs.writeShellScript "waybar-timer-poll" ''
                  state="''${XDG_RUNTIME_DIR:-/tmp}/waybar-timer.state"
                  if [ ! -f "$state" ]; then
                    printf '{"text":"󰔛","tooltip":"Timer\nLeft-click to set\nRight-click to cancel"}'
                    exit 0
                  fi
                  read -r end label < "$state"
                  now=$(date +%s)
                  remaining=$((end - now))
                  if [ "$remaining" -le 0 ]; then
                    rm -f "$state"
                    ${pkgs.libnotify}/bin/notify-send -u critical "Timer" "⏰ ${label:-Done}"
                    printf '{"text":"󰔛","tooltip":"Timer\nLeft-click to set\nRight-click to cancel"}'
                    exit 0
                  fi
                  h=$((remaining / 3600))
                  m=$(((remaining % 3600) / 60))
                  s=$((remaining % 60))
                  if [ "$h" -gt 0 ]; then
                    text=$(printf "󰔛 %d:%02d:%02d" "$h" "$m" "$s")
                  else
                    text=$(printf "󰔛 %d:%02d" "$m" "$s")
                  fi
                  tip=$(printf "Timer: %s\nRight-click to cancel" "${label:-running}")
                  printf '{"text":"%s","tooltip":"%s"}' "$text" "$tip"
                '';
                on-click = pkgs.writeShellScript "waybar-timer-set" ''
                  choice=$(printf '%s\n' \
                    "1 min" "3 min" "5 min" "10 min" "15 min" \
                    "20 min" "25 min" "30 min" "45 min" "1 hour" \
                    "1.5 hours" "2 hours" "Custom…" \
                    | rofi -dmenu -p "Timer" -i -no-custom)
                  [ -z "$choice" ] && exit 0

                  case "$choice" in
                    *Custom*)
                      input=$(rofi -dmenu -p "Duration (e.g. 90s, 10m, 2h)" -i)
                      [ -z "$input" ] && exit 0
                      ;;
                    *hour*)  input="$(echo "$choice" | awk '{print $1}')h" ;;
                    *hours*) input="$(echo "$choice" | awk '{print $1}')h" ;;
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
                    ${pkgs.libnotify}/bin/notify-send "Timer" "Invalid duration: $input"
                    exit 0
                  fi

                  end=$(( $(date +%s) + total ))
                  state="''${XDG_RUNTIME_DIR:-/tmp}/waybar-timer.state"
                  printf '%s %s\n' "$end" "$input" > "$state"
                  ${pkgs.libnotify}/bin/notify-send "Timer" "Started: $input"
                '';
                on-click-right = pkgs.writeShellScript "waybar-timer-cancel" ''
                  state="''${XDG_RUNTIME_DIR:-/tmp}/waybar-timer.state"
                  if [ -f "$state" ]; then
                    rm -f "$state"
                    ${pkgs.libnotify}/bin/notify-send "Timer" "Cancelled"
                  fi
                '';
              };

              "custom/powermenu" = {
                format = "󰐥";
                return-type = "json";
                exec = ''echo '{"text":"󰐥","tooltip":"Power menu"}'  '';
                interval = 86400;
                on-click = pkgs.writeShellScript "waybar-powermenu" ''
                  choice=$(rofi -dmenu -p "Power" -no-custom -i <<EOF
                  ⏻  Shutdown
                   Reboot
                   Suspend
                   Logout
                  EOF
                  )
                  case "$choice" in
                    *Shutdown*) systemctl poweroff ;;
                    *Reboot*)   systemctl reboot ;;
                    *Suspend*)  systemctl suspend ;;
                    *Logout*)   niri msg action quit ;;
                  esac
                '';
                on-click-right = "set-wallpaper.sh";
              };
            } // (lib.optionalAttrs ((osConfig.services'.desktop.displays or []) != []) {
              output = map (d: d.name) (filter (d: !d.auxiliary) osConfig.services'.desktop.displays);
            }));

            bottomBar = ({
              layer = "top";
              position = "bottom";
              height = 36;
              smooth-scrolling-threshold = 5;

              modules-left = ["niri/workspaces" "custom/obsidian" "custom/thunar" "custom/tauon" "custom/whatsapp" "custom/gkeep" "custom/gcal" "custom/gphotos" "custom/thunderbird" "custom/windows"];
              modules-center = [];
              modules-right = [];

              "custom/thunar" = {
                format = "📁";
                tooltip = true;
                tooltip-format = "Thunar";
                on-click = "thunar &";
              };
              "custom/thunderbird" = {
                format = "📧";
                tooltip = true;
                tooltip-format = "Thunderbird";
                on-click = "thunderbird &";
              };
              "custom/obsidian" = {
                format = "📝";
                tooltip = true;
                tooltip-format = "Obsidian";
                on-click = "obsidian &";
              };
              "custom/tauon" = {
                format = "🎵";
                tooltip = true;
                tooltip-format = "Tauon";
                on-click = "tauon &";
              };
              "custom/whatsapp" = {
                format = "💬";
                tooltip = true;
                tooltip-format = "WhatsApp";
                on-click = "whatsie &";
              };
              "custom/gkeep" = {
                format = "🗒️";
                tooltip = true;
                tooltip-format = "Google Keep";
                on-click = "google-chrome-stable --profile-directory=Default --app-id=eilembjdkfgodjkcjnpgpaenohkicgjd &";
              };
              "custom/gcal" = {
                format = "📅";
                tooltip = true;
                tooltip-format = "Google Calendar";
                on-click = "google-chrome-stable --profile-directory=Default --app-id=kjbdgfilnfhdoflbpgamdcdgpehopbep &";
              };
              "custom/gphotos" = {
                format = "🖼️";
                tooltip = true;
                tooltip-format = "Google Photos";
                on-click = "google-chrome-stable --profile-directory=Default --app-id=ncmjhecbjeaamljdfahankockkkdmedg &";
              };

              "custom/windows" = {
                return-type = "json";
                format = "{}";
                interval = 1;
                exec = pkgs.writeShellScript "waybar-windows" ''
                  focused=$(niri msg -j focused-window 2>/dev/null)
                  if [ -z "$focused" ]; then
                    printf '{"text":"","tooltip":"No windows"}'
                    exit 0
                  fi
                  ws=$(echo "$focused" | ${pkgs.jq}/bin/jq -r '.workspace_id')
                  text=$(niri msg -j windows 2>/dev/null \
                    | ${pkgs.jq}/bin/jq -r --argjson ws "$ws" \
                      'def name_map:
                         sub("dev\\.zed\\.Zed-Nightly"; "zed")
                         | sub("tauonmb"; "tauon")
                         | sub("com\\.ktechpit\\.whatsie"; "whatsapp")
                         | sub("chrome-eilembjdkfgodjkcjnpgpaenohkicgjd-Default"; "google-keep")
                         | sub("google-chrome"; "chrome")
                         | sub("com\\.google\\.Chrome"; "chrome")
                         | sub("org\\.gnome\\.Meld"; "meld")
                         | sub("org\\.inkscape\\.Inkscape"; "inkscape")
                         | sub("org\\.kde\\.kid3"; "kid3")
                         | sub("org\\.pulseaudio\\.pavucontrol"; "pavucontrol")
                         | sub("org\\.qbittorrent\\.qBittorrent"; "qbittorrent")
                         | sub("org\\.shotcut\\.Shotcut"; "shotcut")
                         | sub("com\\.obsproject\\.Studio"; "obs")
                         | sub("dev\\.lizardbyte\\.app\\.Sunshine.*"; "sunshine")
                         | sub("io\\.github\\.waylyrics\\.Waylyrics"; "waylyrics")
                         | sub("io\\.github\\.qarmin\\.czkawka"; "czkawka")
                         | sub("io\\.github\\.qarmin\\.krokiet"; "krokiet")
                         | sub("io\\.github\\.JakubMelka\\.Pdf4qt.*"; "pdf4qt")
                         | sub("com\\.github\\.qarmin\\.czkawka"; "czkawka")
                         | sub("org\\.gnome\\.Screenshot"; "screenshot")
                         | sub("OneDriveGUI"; "onedrive")
                         | sub("wemeetapp"; "wemeet")
                         | sub("startcenter"; "libreoffice")
                         | sub("Handy"; "handy")
                         | sub("org\\..*\\."; "")
                         | sub("com\\..*\\."; "")
                         | sub("io\\..*\\."; "")
                         | sub("dev\\..*\\."; "")
                         | sub(".*\\."; "");
                       map(select(.workspace_id == $ws))
                       | sort_by(.is_focused) | reverse
                       | map(
                           (if .is_focused then "▸" else " " end)
                           + " " + ((.app_id // "unknown") | name_map)
                           + ": "
                           + ((.title // "[Untitled]") | .[0:20])
                         ) | join("  │  ")
                       | if . == "" then "" else . end')
                  tip=$(niri msg -j windows 2>/dev/null \
                    | ${pkgs.jq}/bin/jq -r --argjson ws "$ws" \
                      'def name_map:
                         sub("dev\\.zed\\.Zed-Nightly"; "Zed")
                         | sub("tauonmb"; "Tauon")
                         | sub("com\\.ktechpit\\.whatsie"; "Whatsapp")
                         | sub("chrome-eilembjdkfgodjkcjnpgpaenohkicgjd-Default"; "Google-keep")
                         | sub("google-chrome"; "Chrome")
                         | sub("org\\..*\\."; "")
                         | sub(".*\\."; "");
                       map(select(.workspace_id == $ws))
                       | sort_by(.is_focused) | reverse
                       | .[]
                       | (if .is_focused then "▸ " else "  " end)
                         + ((.app_id // "unknown") | name_map) + ": " + (.title // "[Untitled]")')
                  text_escaped=$(printf '%s' "$text" | sed 's/\\/\\\\/g; s/"/\\"/g')
                  tip_escaped=$(printf '%s' "$tip" | sed 's/\\/\\\\/g; s/"/\\"/g; s/$/\\n/' | tr -d '\n' | sed 's/\\n$//')
                  printf '{"text":"%s","tooltip":"%s"}' "$text_escaped" "$tip_escaped"
                '';
                on-click = pkgs.writeShellScript "waybar-windows-pick" ''
                  focused=$(niri msg -j focused-window 2>/dev/null)
                  [ -z "$focused" ] && exit 0
                  ws=$(echo "$focused" | ${pkgs.jq}/bin/jq -r '.workspace_id')
                  niri msg -j windows 2>/dev/null \
                    | ${pkgs.jq}/bin/jq -r --argjson ws "$ws" \
                      'def name_map:
                         sub("dev\\.zed\\.Zed-Nightly"; "zed")
                         | sub("tauonmb"; "tauon")
                         | sub("com\\.ktechpit\\.whatsie"; "whatsapp")
                         | sub("chrome-eilembjdkfgodjkcjnpgpaenohkicgjd-Default"; "google-keep")
                         | sub("google-chrome"; "chrome")
                         | sub("com\\.google\\.Chrome"; "chrome")
                         | sub("org\\.gnome\\.Meld"; "meld")
                         | sub("org\\.inkscape\\.Inkscape"; "inkscape")
                         | sub("org\\.kde\\.kid3"; "kid3")
                         | sub("org\\.pulseaudio\\.pavucontrol"; "pavucontrol")
                         | sub("org\\.qbittorrent\\.qBittorrent"; "qbittorrent")
                         | sub("org\\.shotcut\\.Shotcut"; "shotcut")
                         | sub("com\\.obsproject\\.Studio"; "obs")
                         | sub("dev\\.lizardbyte\\.app\\.Sunshine.*"; "sunshine")
                         | sub("io\\.github\\.waylyrics\\.Waylyrics"; "waylyrics")
                         | sub("io\\.github\\.qarmin\\.czkawka"; "czkawka")
                         | sub("io\\.github\\.qarmin\\.krokiet"; "krokiet")
                         | sub("io\\.github\\.JakubMelka\\.Pdf4qt.*"; "pdf4qt")
                         | sub("com\\.github\\.qarmin\\.czkawka"; "czkawka")
                         | sub("org\\.gnome\\.Screenshot"; "screenshot")
                         | sub("OneDriveGUI"; "onedrive")
                         | sub("wemeetapp"; "wemeet")
                         | sub("startcenter"; "libreoffice")
                         | sub("Handy"; "handy")
                         | sub("org\\..*\\."; "")
                         | sub("com\\..*\\."; "")
                         | sub("io\\..*\\."; "")
                         | sub("dev\\..*\\."; "")
                         | sub(".*\\."; "");
                       map(select(.workspace_id == $ws))
                       | sort_by(.is_focused) | reverse
                       | .[]
                       | (if .is_focused then "▸ " else "  " end)
                         + (.title // "[Untitled]")
                         + "\t" + (.id|tostring)' \
                    | rofi -dmenu -p "Windows" -i -no-custom -theme rofi-cyberpunk-bottom 2>/dev/null \
                    | while IFS=$'\t' read -r _ id; do
                        [ -n "$id" ] && niri msg action focus-window --id "$id" 2>/dev/null
                      done
                '';
              };

              "niri/workspaces" = {
                format = "{index} {name}";
              };
            } // (lib.optionalAttrs ((osConfig.services'.desktop.displays or []) != []) {
              output = map (d: d.name) (filter (d: !d.auxiliary) osConfig.services'.desktop.displays);
            }));
          };
        };
      })
    ]);
  }
