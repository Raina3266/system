# Walker launcher with cyberpunk theme.
#
# Replaces rofi — Walker is a GTK4 Wayland launcher that supports
# click-outside-to-close (unlike rofi on Wayland). Uses elephant's
# built-in clipboard/todo/bitwarden/files providers instead of custom
# shell scripts.
{ pkgs, ... }:
{
  programs.walker = {
    enable = true;
    runAsService = true;
    config = {
      theme = "cyberpunk";
      click_to_close = true;
      close_when_open = true;
      single_click_activation = true;
      # Layer-shell anchoring: keep the default fullscreen overlay (all
      # four anchors) so click-to-close works — clicks on the empty area
      # around the box-wrapper dismiss walker. The box-wrapper itself is
      # nudged to the top-right corner via CSS margins in the theme.
      shell = {
        exclusive_zone = -1;
        layer = "overlay";
        anchor_top = true;
        anchor_bottom = true;
        anchor_left = true;
        anchor_right = true;
      };
      placeholders = {
        "default".input = "Search";
        "default".list = "No Results";
        clipboard.input = "Clipboard";
        clipboard.list = "Clipboard is empty";
        todo.input = "Add or search a task…";
        todo.list = "No tasks";
        bluetooth.input = "Bluetooth";
        bluetooth.list = "No devices found";
        bitwarden.input = "Search vault…";
        bitwarden.list = "No matching entries";
        windows.input = "Search windows…";
        windows.list = "No open windows";
        files.input = "Search files…";
        files.list = "No files found";
        symbols.input = "Search symbols…";
        symbols.list = "No symbols found";
        unicode.input = "Search unicode…";
        unicode.list = "No characters found";
        runner.input = "Run command…";
        runner.list = "No matching commands";
        playerctl.input = "Search players…";
        playerctl.list = "No media players";
        wireplumber.input = "Search audio devices…";
        wireplumber.list = "No audio devices";
        nirisessions.input = "Search sessions…";
        nirisessions.list = "No sessions defined";
      };
      providers.prefixes = [
        {
          provider = "clipboard";
          prefix = ":";
        }
        {
          provider = "todo";
          prefix = "!";
        }
        {
          provider = "bluetooth";
          prefix = "bt:";
        }
        {
          provider = "bitwarden";
          prefix = "pw:";
        }
        {
          provider = "providerlist";
          prefix = ";";
        }
        {
          provider = "websearch";
          prefix = "@";
        }
        {
          provider = "calc";
          prefix = "=";
        }
        {
          provider = "symbols";
          prefix = ".";
        }
        {
          provider = "unicode";
          prefix = "u:";
        }
        {
          provider = "files";
          prefix = "/";
        }
        {
          provider = "windows";
          prefix = "$";
        }
        {
          provider = "runner";
          prefix = ">";
        }
        {
          provider = "playerctl";
          prefix = "mp:";
        }
        {
          provider = "wireplumber";
          prefix = "au:";
        }
        {
          provider = "nirisessions";
          prefix = "ses:";
        }
      ];
    };
    # Elephant provider configuration (merged into elephant's config).
    elephant = {
      # Keep clipboard history tidy: prune entries older than 3 days
      # (4320 minutes), matching the previous cliphist behaviour.
      provider.clipboard.settings = {
        auto_cleanup = 4320;
        max_items = 1000;
      };
      # Power menu — replaces the old walker-dmenu powermenu script.
      # Invoked via `walker -m menus:power`.
      provider.menus.toml."power" = {
        name = "power";
        name_pretty = "Power";
        icon = "system-shutdown";
        action = "sh -c '%VALUE%'";
        entries = [
          {
            text = "   Shutdown";
            value = "systemctl poweroff";
          }
          {
            text = "   Reboot";
            value = "systemctl reboot";
          }
          {
            text = "   Suspend";
            value = "systemctl suspend";
          }
          {
            text = "   Logout";
            value = "niri msg action quit";
          }
        ];
      };
      # Wi-Fi menu — dynamic Lua menu that scans available networks
      # via nmcli and connects on selection. Invoked via
      # `walker -m menus:wifi`.
      provider.menus.lua."wifi" = ''
        Name = "wifi"
        NamePretty = "Wi-Fi"
        Icon = "network-wireless"
        Action = "sh -c '%VALUE%'"
        HideFromProviderlist = false
        Description = "Connect to a Wi-Fi network"
        SearchName = true

        function GetEntries()
          local entries = {}

          -- Rescan and get current connection
          os.execute("nmcli device wifi rescan 2>/dev/null")
          local current = ""
          local h = io.popen("nmcli -t -f active,ssid dev wifi 2>/dev/null | grep '^yes:' | cut -d: -f2")
          if h then
            current = h:read("*l") or ""
            h:close()
          end

          -- List available networks (sorted by signal, deduplicated)
          local h = io.popen("nmcli -t -f ssid,signal,security dev wifi 2>/dev/null | sort -t: -k2 -nr | awk -F: '!seen[$1]++' | head -20")
          if h then
            for line in h:lines() do
              local ssid, signal, security = line:match("^([^:]*):([^:]*):(.*)$")
              if ssid and ssid ~= "" then
                local marker = " "
                if ssid == current then marker = "✓" end
                local bars = "    "
                local sig = tonumber(signal) or 0
                if sig >= 80 then bars = "████"
                elseif sig >= 60 then bars = "███ "
                elseif sig >= 40 then bars = "██  "
                elseif sig >= 20 then bars = "█   "
                end
                local sec = ""
                if security and security ~= "" then sec = " [" .. security .. "]" end
                local text = marker .. "  " .. bars .. "  " .. ssid .. sec
                local value = "nmcli device wifi connect \"" .. ssid .. "\" 2>/dev/null && notify-send 'Wi-Fi' 'Connected to " .. ssid .. "' || (notify-send 'Wi-Fi' 'Connecting to " .. ssid .. "...'; nm-connection-editor &)"
                table.insert(entries, {
                  Text = text,
                  Subtext = "signal " .. signal .. "%",
                  Value = value,
                })
              end
            end
            h:close()
          end

          if #entries == 0 then
            table.insert(entries, {
              Text = "No networks found",
              Subtext = "Try rescanning",
              Value = "nmcli device wifi rescan 2>/dev/null",
            })
          end

          return entries
        end
      '';
    };
    themes =
      let
        base = builtins.readFile ./theme/walker-cyberpunk.css;
        layoutTopRight = builtins.readFile ./theme/walker-layout-top-right.xml;
        layoutBottom = builtins.readFile ./theme/walker-layout-bottom.xml;
      in
      {
        # Default theme: top-right dropdown, sitting just under the top waybar.
        # The layout XML sets valign=start halign=end on the box-wrapper so it
        # actually anchors to the top-right (CSS margins alone can't override
        # GTK4 alignment properties set in the default layout).
        cyberpunk = {
          style = base;
          layouts."layout" = layoutTopRight;
        };
        # Bottom-center dropup, sitting just above the bottom waybar.
        cyberpunk-bottom = {
          style = base;
          layouts."layout" = layoutBottom;
        };
      };
  };

  home.packages = with pkgs; [
    wtype # Wayland typing — used by bitwarden provider's autotype
  ];
}
