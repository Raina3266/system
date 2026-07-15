{
  pkgs,
  ...
}:
{
  imports = [
    ./waybar.nix
  ];

  xdg.configFile."niri/config.kdl".source = ./config.kdl;

  # Walker launcher with cyberpunk theme.
  # Replaces rofi — Walker is a GTK4 Wayland launcher that supports
  # click-outside-to-close (unlike rofi on Wayland). Uses elephant's
  # built-in clipboard/todo providers instead of custom shell scripts.
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
        { provider = "clipboard"; prefix = ":"; }
        { provider = "todo"; prefix = "!"; }
        { provider = "bluetooth"; prefix = "bt:"; }
        { provider = "bitwarden"; prefix = "pw:"; }
        { provider = "providerlist"; prefix = ";"; }
        { provider = "websearch"; prefix = "@"; }
        { provider = "calc"; prefix = "="; }
        { provider = "symbols"; prefix = "."; }
        { provider = "unicode"; prefix = "u:"; }
        { provider = "files"; prefix = "/"; }
        { provider = "windows"; prefix = "$"; }
        { provider = "runner"; prefix = ">"; }
        { provider = "playerctl"; prefix = "mp:"; }
        { provider = "wireplumber"; prefix = "au:"; }
        { provider = "nirisessions"; prefix = "ses:"; }
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
          { text = "   Shutdown";  value = "systemctl poweroff"; }
          { text = "   Reboot";    value = "systemctl reboot"; }
          { text = "   Suspend";   value = "systemctl suspend"; }
          { text = "   Logout";    value = "niri msg action quit"; }
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
    themes = let
      base = builtins.readFile ./themes/walker-cyberpunk.css;
      layoutTopRight = builtins.readFile ./themes/walker-layout-top-right.xml;
      layoutBottom = builtins.readFile ./themes/walker-layout-bottom.xml;
    in {
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

  # Configure rbw (Bitwarden CLI) to use the GTK pinentry for
  # master-password prompts. Elephant's bitwarden provider calls rbw
  # under the hood, so this also covers vault unlocks from walker.
  # Run `rbw config email your@email@example.com` to set your email.
  xdg.configFile."rbw/config.json".text = builtins.toJSON {
    email = "cgl0326@outlook.com";
    pinentry = "pinentry-gtk-2";
  };

  # Tools invoked by niri binds in config.kdl.
  home.packages = with pkgs; [
    brightnessctl  # F5/F6 brightness keys
    rbw            # Bitwarden CLI — used by elephant's bitwarden provider
    wtype          # Wayland typing — used by bitwarden provider's autotype
    pinentry-gtk2  # PIN prompt for rbw login (GTK2 works under Wayland)
    wob            # Wayland overlay progress bar — used by the timer
  ];

  # wob daemon — reads integer percentages from $XDG_RUNTIME_DIR/wob.sock
  # and renders an overlay progress bar. The timer scripts write to this
  # FIFO to visualize the countdown. See scripts/default.nix.
  systemd.user.services.wob = {
    Unit = {
      Description = "wob — Wayland overlay bar";
      ConditionEnvironment = [ "WAYLAND_DISPLAY" ];
      PartOf = [ "graphical-session.target" ];
    };
    Service = {
      ExecStart = "${pkgs.wob}/bin/wob";
      Restart = "on-failure";
      RestartSec = 3;
    };
    Install = { WantedBy = [ "graphical-session.target" ]; };
  };

  programs'.waybar.enable = true;

  # Configure fcitx5 input methods: English (keyboard-gb) + Chinese (pinyin).
  xdg.configFile."fcitx5/profile".text = ''
    [Groups/0]
    Name="Default"
    Default Layout=gb
    DefaultIM=keyboard-gb

    [Groups/0/Items/0]
    Name=keyboard-gb
    Layout=

    [Groups/0/Items/1]
    Name=pinyin
    Layout=

    [GroupOrder]
    0="Default"
  '';

  # Cyberpunk fcitx5 theme (matches waybar/walker palette).
  xdg.dataFile."fcitx5/themes/cyberpunk/theme.conf".source = ./themes/fcitx5-cyberpunk.conf;

  # Use the cyberpunk theme for the classic UI.
  xdg.configFile."fcitx5/conf/classicui.conf".text = ''
    Vertical Center=False
    PerScreenDPI=True
    UseDarkTheme=False
    Theme=cyberpunk
  '';

  home.pointerCursor = {
    enable = true;
    package = pkgs.bibata-cursors;
    name = "Bibata-Modern-Classic";
    size = 24;
    gtk.enable = true;
    x11.enable = true;
  };
}
