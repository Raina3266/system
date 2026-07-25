# Wi-Fi menu: scan and connect to networks via nmcli.
# Actions: Return=connect | Ctrl+D=disconnect (current only) | Ctrl+F=forget (saved networks)
{ pkgs }:
''
  Name = "wifi"
  NamePretty = "Wi-Fi"
  Icon = "network-wireless"
  Action = "sh -c '%VALUE%'"
  HideFromProviderlist = false
  Description = "Connect to a Wi-Fi network"
  SearchName = true

  function GetEntries()
    local entries = {}

    -- Rescan networks and get current connection
    os.execute("nmcli device wifi rescan 2>/dev/null")
    local current = ""
    local h = io.popen("nmcli -t -f active,ssid dev wifi 2>/dev/null | grep '^yes:' | cut -d: -f2")
    if h then
      current = h:read("*l") or ""
      h:close()
    end

    -- Detect Wi-Fi device name (default: wlan0)
    local device = "wlan0"
    local h = io.popen("nmcli -t -f DEVICE,TYPE dev 2>/dev/null | grep ':wifi$' | cut -d: -f1 | head -1")
    if h then
      local d = h:read("*l") or ""
      if d ~= "" then device = d end
      h:close()
    end

    -- Build set of saved SSIDs (filter by TYPE=wifi)
    local saved = {}
    local h = io.popen("nmcli -t -f NAME,TYPE connection show 2>/dev/null | grep ':wifi$' | cut -d: -f1")
    if h then
      for line in h:lines() do
        saved[line] = true
      end
      h:close()
    end

    -- List networks: sorted by signal, deduplicated, top 20
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
            -- No --ask: runs headless, so nmcli delegates to nm-applet secret agent
            -- for password prompts (see ../default.nix)
            value = "nmcli device wifi connect \"" .. ssid .. "\" 2>/dev/null && notify-send 'Wi-Fi' 'Connected to " .. ssid .. "' || notify-send 'Wi-Fi' 'Failed to connect to " .. ssid .. "'"
          end

          local actions = {}
          -- forget: available on saved networks (uses connection profile name)
          if saved[ssid] then
            actions.forget = "nmcli connection delete \"" .. ssid .. "\" 2>/dev/null && notify-send 'Wi-Fi' 'Forgot \"" .. ssid .. "\"'"
          end
          -- disconnect: only on current network
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
''
