{
  pkgs,
  ...
}:
{
  # Tools used by waybar custom modules.
  home.packages = with pkgs; [
    waybar-lyric
    xwayland-satellite
    wl-clipboard
    cliphist
    rofi
    btop
    lm_sensors
    nvitop
  ];

  programs.waybar = {
    enable = true;

    settings = [
      {
        layer = "top";
        position = "top";
        height = 34;
        exclusive = true;
        gtk-layer-shell = true;
        passthrough = false;
        fixed-center = true;
        reload_style_on_change = true;
        margin = "4px 2px";

        # ── Left: screen switcher + system metrics ────────────────────
        modules-left = [
          "custom/screens"
          "cpu"
          "memory"
          "disk"
          "temperature"
          "custom/gpu"
          "network"
        ];

        # ── Center: lyrics (if playing) else clock + calendar ──────────
        modules-center = [
          "custom/lyrics"
          "clock"
        ];

        # ── Right: settings dropdown, clipboard, todo, timer ───────────
        modules-right = [
          "custom/timer"
          "custom/todo"
          "custom/clipboard"
          "custom/settings"
          "tray"
        ];

        # ── Screen switcher (cycles niri monitors) ────────────────────
        "custom/screens" = {
          format = "󰍹";
          tooltip = true;
          tooltip-format = "Cycle monitor focus\nLeft-click: focus next monitor\nRight-click: move column to next monitor";
          on-click = "niri msg action focus-monitor-right";
          on-click-right = "niri msg action move-column-to-monitor-right";
        };

        # ── CPU ────────────────────────────────────────────────────────
        cpu = {
          format = "󰍛 {usage}%";
          tooltip = true;
          interval = 2;
        };

        # ── RAM ────────────────────────────────────────────────────────
        memory = {
          format = "󰾆 {percentage}%";
          tooltip-format = "{used:0.1f}G / {total:0.1f}G";
          interval = 5;
        };

        # ── Disk ──────────────────────────────────────────────────────
        disk = {
          path = "/";
          format = "󰆓 {percentage_used}%";
          tooltip-format = "{used} / {total} ({percentage_used}%)";
          interval = 30;
        };

        # ── Temperature (sensors) ─────────────────────────────────────
        temperature = {
          hwmon-path = "";
          thermal-zone = 0;
          critical-threshold = 80;
          format-critical = "󰔅 {temperatureC}°C";
          format = "󰔘 {temperatureC}°C";
          tooltip-format = "Sensor: {chip}";
          interval = 5;
        };

        # ── GPU (NVIDIA via nvitop, fallback to Intel) ────────────────
        "custom/gpu" = {
          format = "󰢮 {}";
          tooltip = true;
          interval = 3;
          exec = pkgs.writeShellScript "waybar-gpu" ''
            # Try NVIDIA first
            if command -v nvitop >/dev/null 2>&1; then
              util=$(nvidia-smi --query-gpu=utilization.gpu --format=csv,noheader,nounits 2>/dev/null | head -1)
              if [ -n "$util" ]; then
                echo "''${util}%"
                exit 0
              fi
            fi
            # Fallback: intel_gpu_top is too heavy; just report N/A
            echo "—"
          '';
        };

        # ── Network ───────────────────────────────────────────────────
        network = {
          interval = 5;
          format-wifi = "󰤨 {essid}";
          format-ethernet = "󰈀 {ipaddr}";
          format-disconnected = "󰤭";
          tooltip-format-wifi = "{essid} ({signalStrength}dBm)\n{ipaddr}";
          tooltip-format-ethernet = "{ifname}: {ipaddr}";
        };

        # ── Lyrics (centered, hides when nothing plays) ───────────────
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

        # ── Clock + calendar (centered, hidden while lyrics play) ─────
        clock = {
          format = "󰃭 {:%H:%M  %a %d %b}";
          format-alt = " {:%H:%M:%S}";
          tooltip-format = "<tt>{calendar}</tt>";
          calendar = {
            mode = "year";
            mode-mon-col = 3;
            weeks-pos = "right";
            on-scroll = 1;
            on-click-right = "mode";
            format = {
              months = ''<span color='#ff7edb'><b>{}</b></span>'';
              days = ''<span color='#cbe3e7'>{}</span>'';
              weeks = ''<span color='#5c6776'><b>W{}</b></span>'';
              weekdays = ''<span color='#f29e74'><b>{}</b></span>'';
              today = ''<span color='#ff3333'><b><u>{}</u></b></span>'';
            };
          };
          actions = {
            on-click-right = "mode";
            on-click-forward = "tz_up";
            on-click-backward = "tz_down";
            on-scroll-up = "shift_up";
            on-scroll-down = "shift_down";
          };
        };

        # ── Timer (simple countdown via shell) ────────────────────────
        "custom/timer" = {
          format = "󰔛 {}";
          tooltip = true;
          tooltip-format = "Left-click: start 5m timer\nMiddle-click: start 25m (pomodoro)\nRight-click: cancel timer";
          exec = pkgs.writeShellScript "waybar-timer-status" ''
            state="''${XDG_RUNTIME_DIR:-/tmp}/waybar-timer"
            if [ -f "$state" ]; then
              target=$(cat "$state")
              now=$(date +%s)
              left=$((target - now))
              if [ "$left" -le 0 ]; then
                notify-send -u critical "Timer" "⏰ Time's up!"
                rm -f "$state"
                echo "done"
              else
                printf '%dm%02ds\n' $((left / 60)) $((left % 60))
              fi
            else
              echo "—"
            fi
          '';
          interval = 1;
          on-click = pkgs.writeShellScript "waybar-timer-start" ''
            state="''${XDG_RUNTIME_DIR:-/tmp}/waybar-timer"
            echo $(( $(date +%s) + 300 )) > "$state"
          '';
          on-click-middle = pkgs.writeShellScript "waybar-timer-pomodoro" ''
            state="''${XDG_RUNTIME_DIR:-/tmp}/waybar-timer"
            echo $(( $(date +%s) + 1500 )) > "$state"
          '';
          on-click-right = pkgs.writeShellScript "waybar-timer-cancel" ''
            rm -f "''${XDG_RUNTIME_DIR:-/tmp}/waybar-timer"
          '';
        };

        # ── Todo list (stored in XDG_RUNTIME_DIR, rofi picker) ─────────
        "custom/todo" = {
          format = "󰄬 {}";
          tooltip = true;
          tooltip-format = "Left-click: add item\nRight-click: list / remove";
          exec = pkgs.writeShellScript "waybar-todo-count" ''
            f="''${XDG_RUNTIME_DIR:-/tmp}/waybar-todo"
            if [ -f "$f" ]; then
              n=$(grep -c . "$f" 2>/dev/null || echo 0)
              echo "$n"
            else
              echo "0"
            fi
          '';
          interval = 5;
          on-click = pkgs.writeShellScript "waybar-todo-add" ''
            f="''${XDG_RUNTIME_DIR:-/tmp}/waybar-todo"
            item=$(rofi -dmenu -p "Add todo" -l 0 2>/dev/null)
            [ -n "$item" ] && echo "$item" >> "$f"
          '';
          on-click-right = pkgs.writeShellScript "waybar-todo-list" ''
            f="''${XDG_RUNTIME_DIR:-/tmp}/waybar-todo"
            if [ ! -f "$f" ] || [ ! -s "$f" ]; then
              notify-send "Todo" "No items."
              exit 0
            fi
            sel=$(rofi -dmenu -p "Remove" -no-custom < "$f" 2>/dev/null)
            [ -n "$sel" ] && grep -Fxv "$sel" "$f" > "$f.tmp" && mv "$f.tmp" "$f"
          '';
        };

        # ── Clipboard history (cliphist + rofi) ───────────────────────
        "custom/clipboard" = {
          format = "󰆏";
          tooltip = true;
          tooltip-format = "Clipboard history";
          on-click = pkgs.writeShellScript "waybar-clipboard" ''
            selection=$(cliphist list 2>/dev/null | rofi -dmenu -p "Clipboard" -i 2>/dev/null)
            [ -n "$selection" ] && cliphist decode <<< "$selection" | wl-copy
          '';
        };

        # ── Settings dropdown (rofi menu for wifi/bt/battery/etc) ─────
        "custom/settings" = {
          format = "󰒓";
          tooltip = true;
          tooltip-format = "Settings menu";
          on-click = pkgs.writeShellScript "waybar-settings" ''
            choice=$(rofi -dmenu -p "Settings" -no-custom -i <<EOF
            󰤨  Wi-Fi
            󰂯  Bluetooth
            󰁹  Battery
            󰕾  Volume
            󰖩  Display
            󰒃  Power
            EOF
            )
            case "$choice" in
              *Wi-Fi*)      ${pkgs.networkmanagerapplet}/bin/nm-connection-editor & ;;
              *Bluetooth*)  ${pkgs.blueman}/bin/blueman-manager & ;;
              *Battery*)    ${pkgs.gnome-power-manager}/bin/gnome-power-statistics & ;;
              *Volume*)     ${pkgs.pavucontrol}/bin/pavucontrol & ;;
              *Display*)    ${pkgs.gnome-control-center}/bin/gnome-control-center display & ;;
              *Power*)      ${pkgs.gnome-control-center}/bin/gnome-control-center power & ;;
            esac
          '';
        };

        # ── Tray ──────────────────────────────────────────────────────
        tray = {
          show-passive-items = true;
          spacing = 10;
        };
      }
    ];

    # ── Cyberpunk theme ──────────────────────────────────────────────
    style = ''
      * {
        font-family: "JetBrains Mono Nerd Font", "Symbols Nerd Font Mono", "Noto Sans CJK SC", "Noto Sans CJK JP", monospace;
        font-size: 13px;
        font-weight: 600;
      }

      window#waybar {
        background-color: rgba(10, 10, 20, 0.85);
        color: #cbe3e7;
        border-bottom: 2px solid #ff7edb;
        box-shadow: 0 0 20px rgba(255, 126, 219, 0.3);
      }

      /* ── Module base ─────────────────────────────────────────────── */
      #workspaces,
      #cpu, #memory, #disk, #temperature, #custom-gpu, #network,
      #clock, #custom-lyrics,
      #custom-timer, #custom-todo, #custom-clipboard, #custom-settings,
      #tray {
        background-color: rgba(20, 20, 40, 0.7);
        margin: 0 3px;
        padding: 0 10px;
        border-radius: 6px;
        border: 1px solid rgba(255, 126, 219, 0.2);
      }

      /* ── Left: neon cyan/pink ────────────────────────────────────── */
      #custom-screens {
        color: #ff7edb;
        text-shadow: 0 0 8px rgba(255, 126, 219, 0.6);
      }
      #cpu {
        color: #7afcff;
        text-shadow: 0 0 6px rgba(122, 252, 255, 0.5);
      }
      #memory {
        color: #7afcff;
        text-shadow: 0 0 6px rgba(122, 252, 255, 0.5);
      }
      #disk {
        color: #7afcff;
        text-shadow: 0 0 6px rgba(122, 252, 255, 0.5);
      }
      #temperature {
        color: #f29e74;
        text-shadow: 0 0 6px rgba(242, 158, 116, 0.5);
      }
      #temperature.critical {
        color: #ff3333;
        text-shadow: 0 0 10px rgba(255, 51, 51, 0.7);
        animation: blink 1s steps(2) infinite;
      }
      #custom-gpu {
        color: #7afcff;
        text-shadow: 0 0 6px rgba(122, 252, 255, 0.5);
      }
      #network {
        color: #7afcff;
        text-shadow: 0 0 6px rgba(122, 252, 255, 0.5);
      }
      #network.disconnected {
        color: #ff3333;
        text-shadow: 0 0 8px rgba(255, 51, 51, 0.6);
      }

      /* ── Center: lyrics (green when playing) / clock (purple) ───── */
      #custom-lyrics {
        color: #7afcff;
        text-shadow: 0 0 8px rgba(122, 252, 255, 0.6);
      }
      #custom-lyrics.paused {
        color: #5c6776;
        text-shadow: none;
      }
      #clock {
        color: #ff7edb;
        text-shadow: 0 0 8px rgba(255, 126, 219, 0.6);
      }

      /* ── Right: amber/pink utilities ─────────────────────────────── */
      #custom-timer {
        color: #f29e74;
        text-shadow: 0 0 6px rgba(242, 158, 116, 0.5);
      }
      #custom-todo {
        color: #f29e74;
        text-shadow: 0 0 6px rgba(242, 158, 116, 0.5);
      }
      #custom-clipboard {
        color: #ff7edb;
        text-shadow: 0 0 6px rgba(255, 126, 219, 0.5);
      }
      #custom-settings {
        color: #ff7edb;
        text-shadow: 0 0 6px rgba(255, 126, 219, 0.5);
      }
      #tray {
        color: #cbe3e7;
      }

      /* ── Hover effect ────────────────────────────────────────────── */
      #custom-screens:hover, #cpu:hover, #memory:hover, #disk:hover,
      #temperature:hover, #custom-gpu:hover, #network:hover,
      #clock:hover, #custom-lyrics:hover,
      #custom-timer:hover, #custom-todo:hover, #custom-clipboard:hover,
      #custom-settings:hover, #tray:hover {
        background-color: rgba(255, 126, 219, 0.15);
        border-color: rgba(255, 126, 219, 0.5);
      }

      /* ── Calendar tooltip ────────────────────────────────────────── */
      tooltip {
        background-color: rgba(10, 10, 20, 0.95);
        border: 1px solid #ff7edb;
        border-radius: 6px;
        box-shadow: 0 0 15px rgba(255, 126, 219, 0.3);
      }
      tooltip label {
        color: #cbe3e7;
      }

      @keyframes blink {
        to { color: #1a1a2e; text-shadow: none; }
      }
    '';
  };
}
