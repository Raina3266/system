# Walker launcher with cyberpunk theme.
#
# Replaces rofi — Walker is a GTK4 Wayland launcher that supports
# click-outside-to-close (unlike rofi on Wayland). Uses elephant's
# built-in clipboard/todo/files providers instead of custom
# shell scripts.
{ pkgs, inputs, ... }:
{
  imports = [ inputs.walker.homeManagerModules.default ];

  programs.walker = {
    enable = true;
    runAsService = true;
    config = {
      theme = "cyberpunk";
      click_to_close = true;
      close_when_open = true;
      single_click_activation = true;
      # Hide the F1–F4 quick-activation buttons in the popup.
      hide_quick_activation = true;
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
      providers = {
        # Providers queried by default (empty query) and on launch.
        # Without these, walker shows nothing when opened plain.
        default = [
          "desktopapplications"
          "calc"
          "websearch"
        ];
        empty = [ "desktopapplications" ];
        # Skip the preview pane (right-hand box) for the clipboard
        # provider — the entry list on the left stays as-is.
        ignore_preview = [ "clipboard" ];
        prefixes = [
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
        # Per-provider action keybinds. These are REQUIRED for the todo
        # provider (and others) to function — without them, Enter/Ctrl+D
        # etc. do nothing. The walker module's `config` option replaces
        # its default (imported from resources/config.toml) entirely, so
        # we must re-declare the actions we want here.
        actions = {
          todo = [
            { action = "save"; default = true; bind = "Return"; after = "AsyncClearReload"; }
            { action = "delete"; label = "delete"; bind = "Delete"; after = "AsyncClearReload"; }
            { action = "active"; default = true; bind = "Return"; after = "Nothing"; }
            { action = "inactive"; default = true; bind = "Return"; after = "Nothing"; }
            { action = "done"; label = "done"; bind = "ctrl Return"; after = "Nothing"; }
            { action = "search"; label = "search"; bind = "ctrl a"; after = "AsyncClearReload"; }
          ];
          desktopapplications = [
            { action = "start"; default = true; bind = "Return"; }
            { action = "new_instance"; label = "new instance"; bind = "ctrl Return"; }
            { action = "pin"; bind = "ctrl p"; after = "AsyncReload"; }
            { action = "unpin"; bind = "ctrl p"; after = "AsyncReload"; }
          ];
          clipboard = [
            { action = "copy"; default = true; bind = "Return"; }
            { action = "remove"; bind = "ctrl d"; after = "AsyncClearReload"; }
            { action = "show_images_only"; label = "only images"; bind = "ctrl i"; after = "AsyncClearReload"; }
            { action = "show_text_only"; label = "only text"; bind = "ctrl i"; after = "AsyncClearReload"; }
            { action = "unpin"; bind = "ctrl p"; after = "AsyncClearReload"; }
            { action = "pin"; bind = "ctrl p"; after = "AsyncClearReload"; }
            { action = "edit"; bind = "ctrl o"; }
          ];
          files = [
            { action = "open"; default = true; bind = "Return"; }
            { action = "opendir"; label = "open dir"; bind = "ctrl Return"; }
            { action = "copypath"; label = "copy path"; bind = "ctrl shift c"; }
            { action = "copyfile"; label = "copy file"; bind = "ctrl c"; }
            { action = "localsend"; label = "localsend"; bind = "ctrl l"; }
          ];
          calc = [
            { action = "copy"; default = true; bind = "Return"; }
            { action = "delete"; bind = "ctrl d"; after = "AsyncReload"; }
            { action = "delete_all"; bind = "ctrl shift d"; after = "AsyncReload"; }
            { action = "save"; bind = "ctrl s"; after = "AsyncClearReload"; }
          ];
          websearch = [
            { action = "search"; default = true; bind = "Return"; }
            { action = "open_url"; label = "open url"; default = true; bind = "Return"; }
          ];
          runner = [
            { action = "run"; default = true; bind = "Return"; }
            { action = "runterminal"; label = "run in terminal"; bind = "shift Return"; }
          ];
          symbols = [
            { action = "run_cmd"; label = "select"; default = true; bind = "Return"; }
          ];
          unicode = [
            { action = "run_cmd"; label = "select"; default = true; bind = "Return"; }
          ];
          windows = [
            { action = "activate"; default = true; bind = "Return"; }
          ];
          providerlist = [
            { action = "activate"; default = true; bind = "Return"; after = "ClearReload"; }
          ];
          bluetooth = [
            { action = "find"; bind = "ctrl f"; after = "AsyncClearReload"; }
            { action = "remove"; bind = "ctrl d"; after = "AsyncReload"; }
            { action = "trust"; bind = "ctrl t"; after = "AsyncReload"; }
            { action = "untrust"; bind = "ctrl t"; after = "AsyncReload"; }
            { action = "pair"; bind = "Return"; after = "AsyncReload"; }
            { action = "connect"; default = true; bind = "Return"; after = "AsyncReload"; }
            { action = "disconnect"; default = true; bind = "Return"; after = "AsyncReload"; }
            { action = "power_on"; label = "Power On"; bind = "ctrl e"; after = "AsyncReload"; }
            { action = "power_off"; label = "Power Off"; bind = "ctrl e"; after = "AsyncReload"; }
          ];
          playerctl = [
            { action = "pause"; label = "pause"; bind = "Return"; after = "Nothing"; default = true; }
            { action = "play"; label = "play"; bind = "Return"; after = "Nothing"; default = true; }
            { action = "prev"; label = "prev"; bind = "ctrl p"; after = "Nothing"; }
            { action = "next"; label = "next"; bind = "ctrl n"; after = "Nothing"; }
            { action = "vol_up"; label = "vol+"; bind = "ctrl y"; after = "Nothing"; }
            { action = "vol_down"; label = "vol-"; bind = "ctrl h"; after = "Nothing"; }
            { action = "mute"; label = "mute"; bind = "ctrl m"; after = "Nothing"; }
            { action = "unmute"; label = "unmute"; bind = "ctrl m"; after = "Nothing"; }
            { action = "seek_back"; label = "backward"; bind = "ctrl b"; after = "Nothing"; }
            { action = "seek_forward"; label = "forward"; bind = "ctrl f"; after = "Nothing"; }
          ];
          wireplumber = [
            { action = "increase_volume"; label = "+volume"; bind = "ctrl y"; after = "Nothing"; }
            { action = "decrease_volume"; label = "-volume"; bind = "ctrl n"; after = "Nothing"; }
            { action = "mute"; bind = "ctrl m"; after = "Nothing"; }
            { action = "unmute"; bind = "ctrl m"; after = "Nothing"; }
            { action = "set_default_device"; label = "set default"; bind = "ctrl d"; after = "Nothing"; }
          ];
          nirisessions = [
            { action = "start"; label = "start"; default = true; bind = "Return"; }
            { action = "start_new"; label = "start blank"; bind = "ctrl Return"; }
          ];
          fallback = [
            { action = "menus:open"; label = "open"; after = "Nothing"; }
            { action = "menus:default"; label = "run"; after = "Close"; }
            { action = "menus:parent"; label = "back"; bind = "Escape"; after = "Nothing"; }
            { action = "erase_history"; label = "clear hist"; bind = "ctrl h"; after = "AsyncReload"; }
          ];
          # Per-entry actions for menus. Walker registers the menus
          # provider under the name "menus" (from `elephant listproviders`),
          # NOT "menus:wifi" — so keybinds for custom per-entry actions
          # (disconnect/forget on wifi entries) must live under "menus".
          # These actions only exist on wifi entries, so the buttons only
          # appear there; other menus (power, etc.) are unaffected.
          menus = [
            { action = "disconnect"; label = "disconnect"; bind = "ctrl d"; after = "AsyncClearReload"; }
            { action = "forget"; label = "forget"; bind = "ctrl f"; after = "AsyncClearReload"; }
          ];
          dmenu = [ { action = "select"; default = true; bind = "Return"; } ];
        };
      };
      # Global keybinds — also lost when `config` replaces the default.
      keybinds = {
        close = [ "Escape" ];
        next = [ "Down" ];
        previous = [ "Up" ];
        left = [ "Left" ];
        right = [ "Right" ];
        down = [ "Down" ];
        up = [ "Up" ];
        toggle_exact = [ "ctrl e" ];
        resume_last_query = [ "ctrl r" ];
        page_down = [ "Page_Down" ];
        page_up = [ "Page_Up" ];
        show_actions = [ "alt j" ];
      };
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
      provider.menus.lua."audio-sink" = ''
        Name = "audio-sink"
        NamePretty = "Audio Output"
        Icon = "audio-card"
        Action = "sh -c '%VALUE%'"
        HideFromProviderlist = false
        Description = "Switch audio output device"
        SearchName = true

        function GetEntries()
          local entries = {}
          local sinks = {}

          local h = io.popen("wpctl status 2>/dev/null | sed -n '/. Sinks:/,/. Sources:/p' | grep -P '[0-9]+\\.'")
          if h then
            for line in h:lines() do
              local is_default = line:find("%*") ~= nil
              local id = line:match("(%d+)%.")
              local desc = line:match("%d+%.%s+(.-)%s*%[vol:")
              if id and desc then
                table.insert(sinks, { id = id, desc = desc, is_default = is_default })
              end
            end
            h:close()
          end

          -- Strip the longest common word-prefix shared by all
          -- descriptions (usually the card name), so only the
          -- distinguishing suffix (Speaker, Headphones, HDMI ...) shows.
          if #sinks > 1 then
            local words = {}
            for w in sinks[1].desc:gmatch("%S+") do
              table.insert(words, w)
            end
            local common = #words
            for i = 2, #sinks do
              local w2 = {}
              for w in sinks[i].desc:gmatch("%S+") do
                table.insert(w2, w)
              end
              local j = 0
              while j < common and j < #w2 and words[j + 1] == w2[j + 1] do
                j = j + 1
              end
              common = j
            end
            if common > 0 then
              local prefix = table.concat(words, " ", 1, common) .. " "
              for _, s in ipairs(sinks) do
                if s.desc:sub(1, #prefix) == prefix then
                  s.desc = s.desc:sub(#prefix + 1)
                end
              end
            end
          end

          for _, s in ipairs(sinks) do
            local marker = s.is_default and "✓" or " "
            table.insert(entries, {
              Text = marker .. "  " .. s.desc,
              Value = "wpctl set-default " .. s.id,
            })
          end

          if #entries == 0 then
            table.insert(entries, {
              Text = "No audio sinks found",
              Value = "true",
            })
          end

          return entries
        end
      '';
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
      #
      # Per-entry actions:
      #   - disconnect (ctrl d): only on the currently-connected network
      #   - forget     (ctrl f): on any saved network (connected or not)
      # The default action (Return) connects to the network.
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

          -- Detect the wifi device name (e.g. wlan0) for disconnect.
          local device = "wlan0"
          local h = io.popen("nmcli -t -f DEVICE,TYPE dev 2>/dev/null | grep ':wifi$' | cut -d: -f1 | head -1")
          if h then
            local d = h:read("*l") or ""
            if d ~= "" then device = d end
            h:close()
          end

          -- Build a set of saved SSIDs (connections known to NetworkManager).
          -- nmcli connection show lists saved profiles; the SSID field may
          -- be blank for non-wifi connections, so filter by TYPE=wifi.
          local saved = {}
          local h = io.popen("nmcli -t -f NAME,TYPE connection show 2>/dev/null | grep ':wifi$' | cut -d: -f1")
          if h then
            for line in h:lines() do
              saved[line] = true
            end
            h:close()
          end

          -- List available networks (sorted by signal, deduplicated)
          local h = io.popen("nmcli -t -f ssid,signal,security dev wifi 2>/dev/null | sort -t: -k2 -nr | awk -F: '!seen[$1]++' | head -20")
          if h then
            for line in h:lines() do
              local ssid, signal, security = line:match("^([^:]*):([^:]*):(.*)$")
              if ssid and ssid ~= "" then
                local marker = " "
                local is_current = (ssid == current)
                if is_current then marker = "✓" end
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

                local actions = {}
                -- forget: available on any saved network (connected or not).
                -- Uses the connection profile name, which for wifi is usually
                -- the SSID but may differ; look it up to be safe.
                if saved[ssid] then
                  actions.forget = "nmcli connection delete \"" .. ssid .. "\" 2>/dev/null && notify-send 'Wi-Fi' 'Forgot \"" .. ssid .. "\"'"
                end
                -- disconnect: only on the currently-connected network.
                if is_current then
                  actions.disconnect = "nmcli device disconnect \"" .. device .. "\" 2>/dev/null && notify-send 'Wi-Fi' 'Disconnected from " .. ssid .. "'"
                end

                table.insert(entries, {
                  Text = text,
                  Subtext = "signal " .. signal .. "%",
                  Value = value,
                  Actions = actions,
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
        base = builtins.readFile ../themes/walker-cyberpunk.css;
        layoutTopRight = builtins.readFile ../themes/walker-layout-top-right.xml;
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
      };
  };

  home.packages = with pkgs; [
    wtype # Wayland typing
    fd # files provider (elephant/walker)
  ];
}
