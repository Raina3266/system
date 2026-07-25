# Elephant provider configuration (merged into elephant's config),
# for walker's `elephant` option.
{ pkgs }:
{
  # Keep clipboard history tidy: prune entries older than 3 days
  # (4320 minutes), matching the previous cliphist behaviour.
  provider.clipboard.settings = {
    auto_cleanup = 4320;
    max_items = 1000;
    pinned_on_top = true;
  };
  # Todo provider — task list stored in ~/.cache/elephant/todo.csv.
  # These are the defaults; declared explicitly so they're documented
  # here and can be tweaked. The popup only lists entries when tasks
  # exist — when empty, type a name and press Return to create one.
  provider.todo.settings = {
    # Minutes before/after a scheduled time during which a task is
    # shown as urgent (red). Notifications fire at the scheduled time.
    urgent_time_frame = 30;
    # Lower other players' volume while a task notification plays.
    duck_player_volumes = true;
    # Show creation time in the subtext when no other time info exists.
    show_creation_time = false;
    # Time format for subtext (Go time layout).
    time_format = "01-02 15:04";
    # Categories: prefix a query with `prefix` to file the new task
    # under that category (e.g. `w:fix report`). Cycling an existing
    # task's category is bound to ctrl+y (change_category).
    categories = [
      {
        name = "work";
        prefix = "w:";
      }
      {
        name = "personal";
        prefix = "p:";
      }
    ];
    # Notification shown when a scheduled task's time arrives.
    # %TASK% is replaced with the task's text. (These fields are
    # squashed into the top level of the todo settings upstream, not
    # nested under a "notification" key.)
    title = "Task Due";
    body = "🔔 %TASK%";
  };
  # Audio menu — see ./audio.nix.
  provider.menus.lua."audio" = import ./audio.nix { inherit pkgs; };
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
  # Bluetooth menu — D-Bus (gdbus) based; replaces elephant's built-in
  # bluetooth provider, whose bluetoothctl one-shot "pair" races the
  # persistent bt-agent and hangs at "Pairing...". See bluetooth.nix.
  # Invoked via `walker -m menus:bluetooth`.
  provider.menus.lua."bluetooth" = (import ./bluetooth.nix { inherit pkgs; }).menuLua;

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
            local value = ""
            if saved[ssid] then
              value = "nmcli connection up \"" .. ssid .. "\" 2>/dev/null && notify-send 'Wi-Fi' 'Connected to " .. ssid .. "' || notify-send 'Wi-Fi' 'Failed to connect to " .. ssid .. "'"
            else
              -- No --ask here: nmcli would try to prompt on its own (nonexistent)
              -- stdin/tty and hang/fail silently since this runs headless under
              -- elephant. Omitting --ask makes NetworkManager instead ask any
              -- registered secret agent for the password -- nm-applet (running
              -- as a user service, see ../default.nix) is one, and pops up its
              -- native GTK password dialog.
              value = "nmcli device wifi connect \"" .. ssid .. "\" 2>/dev/null && notify-send 'Wi-Fi' 'Connected to " .. ssid .. "' || notify-send 'Wi-Fi' 'Failed to connect to " .. ssid .. "'"
            end

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
}
