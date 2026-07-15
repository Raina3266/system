{
  pkgs,
  lib,
  config,
  osConfig,
  ...
}:
let
  cfg = config.programs'.waybar;
  scripts = import ./scripts { inherit pkgs; };

  # Outputs to attach the bars to: every non-auxiliary display declared
  # in osConfig.services'.desktop.displays (if any).
  barOutputs = lib.optionalAttrs ((osConfig.services'.desktop.displays or [ ]) != [ ]) {
    output = map (d: d.name) (lib.filter (d: !d.auxiliary) osConfig.services'.desktop.displays);
  };
in
{
  options.programs'.waybar = {
    enable = lib.mkEnableOption "waybar";
    enableNiriIntegration = lib.mkEnableOption "Niri workspace switcher";
  };

  config = lib.mkIf (pkgs.stdenv.isLinux && cfg.enable) (lib.mkMerge [
    {
      home.packages = with pkgs; [
        wl-clipboard
        jq
        playerctl
      ];

      systemd.user.services.waybar = {
        Unit = {
          # Only run under niri — GNOME/Mutter lacks layer-shell support
          # and waybar would crash-loop there.
          ConditionEnvironment = lib.mkForce [ "XDG_CURRENT_DESKTOP=niri" ];
        };
        Service = {
          Restart = lib.mkForce "on-failure";
          RestartSec = 3;
        };
      };
    }

    (lib.mkIf (osConfig != null) {
      programs.waybar = {
        enable = true;
        systemd.enable = true;
        style = ./themes/waybar-cyberpunk.css;

        settings = {
          topBar = ({
            layer = "top";
            position = "top";
            height = 36;
            smooth-scrolling-threshold = 5;

            modules-left = [ "clock" "tray" "group/system" "group/hardware" ];
            modules-center = [ "custom/media" ];
            modules-right = [
              "custom/bitwarden"
              "custom/todo"
              "custom/timer"
              "custom/cliphist"
              "custom/files"
              "custom/bt"
              "custom/wifi"
              "custom/powermenu"
            ];

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
                transition-left-to-right = true;
              };
              modules = [ "temperature" "memory" "cpu" "disk" "network" ];
            };

            "group/system" = {
              orientation = "horizontal";
              drawer = {
                transition-duration = 300;
                transition-left-to-right = true;
              };
              modules = [ "battery" "backlight" "pulseaudio" ];
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
              exec = ''echo '{"text":"󰤨","tooltip":"Wi-Fi networks\nLeft-click to scan & connect"}' '';
              interval = 86400;
              on-click = pkgs.writeShellScript "waybar-wifi" ''
                ${pkgs.walker}/bin/walker -m menus:wifi
              '';
            };

            # Bluetooth — walker's built-in `bluetooth` provider (elephant).
            "custom/bt" = {
              format = "󰂯";
              return-type = "json";
              exec = ''echo '{"text":"󰂯","tooltip":"Bluetooth devices\nLeft-click to scan & connect"}' '';
              interval = 86400;
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
              format-icons = [ "󰁺" "󰁻" "󰁼" "󰁽" "󰁾" "󰁿" "󰂀" "󰂁" "󰂂" "󰁹" ];
              format-plugged = "󰂄 {capacity}%";
              states = {
                warning = 20;
                critical = 10;
              };
              interval = 5;
              on-click = pkgs.writeShellScript "waybar-battery-profile" ''
                choice=$(${pkgs.walker}/bin/walker -t cyberpunk-topleft -d -p "Power Profile" <<EOF
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

            # Clipboard history — walker's `clipboard` provider (elephant).
            # Right-click clears non-pinned history.
            "custom/cliphist" = {
              format = "󰆏";
              return-type = "json";
              exec = ''echo '{"text":"󰆏","tooltip":"Clipboard history\nLeft-click to browse\nRight-click to clear"}' '';
              interval = 86400;
              on-click = pkgs.writeShellScript "waybar-cliphist" ''
                ${pkgs.walker}/bin/walker -m clipboard
              '';
              on-click-right = pkgs.writeShellScript "waybar-cliphist-clear" ''
                ${pkgs.libnotify}/bin/notify-send "Clipboard" "History cleared"
              '';
            };

            # Todo count — see scripts/default.nix for the poll logic.
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

            # Countdown timer — see scripts/default.nix.
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

            # Bitwarden — walker's `bitwarden` provider (elephant + rbw).
            "custom/bitwarden" = {
              format = "󰒃";
              return-type = "json";
              exec = ''echo '{"text":"󰒃","tooltip":"Bitwarden vault\nLeft-click to search"}' '';
              interval = 86400;
              on-click = pkgs.writeShellScript "waybar-bitwarden" ''
                ${pkgs.walker}/bin/walker -m bitwarden
              '';
            };

            # ── Walker provider launcher buttons ──────────────────────────
            # Each button opens walker focused on a specific elephant provider.
            # Icons are Nerd Font glyphs that match each provider's purpose.

            # Files — elephant `files` provider (fd-backed file search).
            "custom/files" = {
              format = "󰥢";
              return-type = "json";
              exec = ''echo '{"text":"󰥢","tooltip":"Files\nLeft-click to search"}' '';
              interval = 86400;
              on-click = pkgs.writeShellScript "waybar-files" ''
                ${pkgs.walker}/bin/walker -m files
              '';
            };

            # ── Media player (center, appears only when playing) ────────
            # Polls playerctl for metadata. Waybar hides the module when the
            # text is empty (hide-empty-text). Left-click: play/pause,
            # right-click: next, scroll up/down: prev/next.
            "custom/media" = {
              format = "{icon} {}";
              format-icons = {
                "Playing" = "▶";
                "Paused"  = "⏸";
                "Stopped" = "⏹";
              };
              return-type = "json";
              exec = pkgs.writeShellScript "waybar-media-poll" ''
                status=$(${pkgs.playerctl}/bin/playerctl status 2>/dev/null)
                if [ -z "$status" ]; then
                  printf '{"text":"","class":"stopped"}'
                  exit 0
                fi
                artist=$(${pkgs.playerctl}/bin/playerctl metadata --format '{{artist}}' 2>/dev/null)
                title=$(${pkgs.playerctl}/bin/playerctl metadata --format '{{title}}' 2>/dev/null)
                # Truncate to keep the bar tidy
                title=$(printf '%.30s' "$title")
                artist=$(printf '%.20s' "$artist")
                if [ -n "$artist" ]; then
                  text="$artist - $title"
                else
                  text="$title"
                fi
                class=$(echo "$status" | tr '[:upper:]' '[:lower:]')
                ${pkgs.jq}/bin/jq -cn --arg text "$text" --arg class "$class" \
                  '{text:$text, class:$class, alt:$class}'
              '';
              interval = 2;
              hide-empty-text = true;
              on-click = "${pkgs.playerctl}/bin/playerctl play-pause";
              on-click-right = "${pkgs.playerctl}/bin/playerctl next";
              on-scroll-up = "${pkgs.playerctl}/bin/playerctl next";
              on-scroll-down = "${pkgs.playerctl}/bin/playerctl previous";
            };

            "custom/powermenu" = {
              format = "󰐥";
              return-type = "json";
              exec = ''echo '{"text":"󰐥","tooltip":"Power menu"}' '';
              interval = 86400;
              # elephant's `menus` provider — defined in
              # programs.walker.elephant.provider.menus.toml in default.nix.
              on-click = pkgs.writeShellScript "waybar-powermenu" ''
                ${pkgs.walker}/bin/walker -m menus:power
              '';
              on-click-right = "set-wallpaper.sh";
            };
          } // barOutputs);

          bottomBar = ({
            layer = "top";
            position = "bottom";
            height = 36;
            smooth-scrolling-threshold = 5;

            modules-left = [
              "niri/workspaces"
              "custom/thunar"
              "custom/thunderbird"
              "custom/gcal"
              "custom/obsidian"
              "custom/tauon"
              "custom/whatsapp"
              "custom/gkeep"
              "custom/gphotos"
              "custom/windows"
            ];
            modules-center = [ ];
            modules-right = [ ];

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

            # Window list — see scripts/default.nix. Walker's `windows`
            # provider is invoked with the bottom dropup theme.
            "custom/windows" = {
              return-type = "json";
              format = "{}";
              interval = 1;
              exec = scripts.windowsPoll;
              on-click = pkgs.writeShellScript "waybar-windows-pick" ''
                ${pkgs.walker}/bin/walker -t cyberpunk-bottom -m windows
              '';
            };

            "niri/workspaces" = {
              format = "{index} {name}";
            };
          } // barOutputs);
        };
      };
    })
  ]);
}
