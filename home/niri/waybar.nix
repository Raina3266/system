{
  pkgs,
  lib,
  config,
  osConfig,
  ...
}: let
  cfg = config.programs'.waybar;
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
        ] ++ (optional cfg.enableLyrics pkgs.waybar-lyric);

        systemd.user.services.cliphist = {
          Unit = {
            Description = "cliphist Wayland clipboard history daemon";
            PartOf = [ "graphical-session.target" ];
            After = [ "graphical-session.target" ];
          };
          Service = {
            ExecStart = "${pkgs.wl-clipboard}/bin/wl-paste --watch ${pkgs.cliphist}/bin/cliphist store";
            Restart = "on-failure";
            RestartSec = 3;
          };
          Install = {
            WantedBy = [ "graphical-session.target" ];
          };
        };

        systemd.user.services.waybar = {
          Service = {
            Restart = lib.mkForce "always";
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

              modules-left = ["clock" "niri/workspaces" "group/hardware"];
              modules-right = ["custom/cliphist" "tray" "custom/bt" "custom/wifi" "group/system" "custom/powermenu"];
              modules-center =
                []
                ++ (optional cfg.enableLyrics "custom/lyrics");

              "niri/workspaces" = {
                format = "●";
              };

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
                tooltip = true;
                tooltip-format = "Wi-Fi networks\nLeft-click to scan & connect";
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
                format = "󰂗";
                tooltip = true;
                tooltip-format = "Bluetooth devices\nLeft-click to scan & connect";
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
                tooltip = true;
                tooltip-format = "Clipboard history\nLeft-click to browse\nRight-click to clear";
                on-click = pkgs.writeShellScript "waybar-cliphist" ''
                  ${pkgs.cliphist}/bin/cliphist list \
                    | ${pkgs.rofi}/bin/rofi -dmenu -p "Clipboard" -i \
                    | ${pkgs.cliphist}/bin/cliphist decode \
                    | ${pkgs.wl-clipboard}/bin/wl-copy
                '';
                on-click-right = pkgs.writeShellScript "waybar-cliphist-clear" ''
                  ${pkgs.cliphist}/bin/cliphist wipe
                  ${pkgs.libnotify}/bin/notify-send "Clipboard" "History cleared"
                '';
              };

              "custom/powermenu" = {
                format = "󰐥";
                tooltip = true;
                tooltip-format = "Power menu";
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
          };
        };
      })
    ]);
  }
