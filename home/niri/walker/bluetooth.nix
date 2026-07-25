# Bluetooth menu via BlueZ D-Bus API (gdbus).
# Uses D-Bus instead of bluetoothctl to avoid pairing agent conflicts that cause
# "Pairing..." hangs with elephant's built-in provider. Works with persistent
# bt-agent (../default.nix) for auto-confirmation.
#
# Modes (persisted in elephant state):
#   list (default) - paired devices: connect/disconnect/forget/scan/power
#   scan           - all devices: pair new devices, see existing ones with state
# Mode persists during filtering. Pairing returns to list mode.
{ pkgs }:
let
  gdbus = "${pkgs.glib}/bin/gdbus";
  notify = "${pkgs.libnotify}/bin/notify-send";
  adapter = "/org/bluez/hci0";

  # btscan: run discovery for N seconds. bluetoothctl --timeout keeps session alive
  # (one-shot gdbus disconnects immediately and finds nothing). Temporary pairing
  # agent doesn't interfere since we never pair during scan.
  # writeShellScriptBin creates bin/ directory structure needed by menu Lua.
  btscan = pkgs.writeShellScriptBin "btscan" ''
    secs="''${1:-6}"
    ${pkgs.bluez}/bin/bluetoothctl --timeout "$secs" scan on >/dev/null 2>&1
  '';

  # btctl: D-Bus command wrapper for menu actions (also used by waybar).
  # writeShellScriptBin for bin/ directory structure.
  btctl = pkgs.writeShellScriptBin "btctl" ''
    set -u
    dev_path() { printf '${adapter}/dev_%s' "$(printf '%s' "$1" | tr ':' '_')"; }

    case "$1" in
      pair)
        # Pair, then trust so the device auto-reconnects, then connect.
        mac="$2"; name="$3"; path=$(dev_path "$mac")
        err=$(${gdbus} call --system --dest org.bluez --object-path "$path" \
              --method org.bluez.Device1.Pair 2>&1)
        rc=$?
        # "Already Exists" = already paired (treat as success)
        if [ $rc -ne 0 ] && ! printf '%s' "$err" | grep -qi 'Already Exists'; then
          ${notify} -u critical "Bluetooth" "Failed to pair $name"
          exit 1
        fi
        ${gdbus} call --system --dest org.bluez --object-path "$path" \
          --method org.freedesktop.DBus.Properties.Set \
          org.bluez.Device1 Trusted "<true>" >/dev/null 2>&1
        ${gdbus} call --system --dest org.bluez --object-path "$path" \
          --method org.bluez.Device1.Connect >/dev/null 2>&1 \
          && ${notify} "Bluetooth" "Paired & connected: $name" \
          || ${notify} "Bluetooth" "Paired: $name (not connected)"
        ;;
      connect)
        mac="$2"; name="$3"; path=$(dev_path "$mac")
        ${gdbus} call --system --dest org.bluez --object-path "$path" \
          --method org.bluez.Device1.Connect >/dev/null 2>&1 \
          && ${notify} "Bluetooth" "Connected: $name" \
          || ${notify} -u critical "Bluetooth" "Failed to connect $name"
        ;;
      disconnect)
        mac="$2"; name="$3"; path=$(dev_path "$mac")
        ${gdbus} call --system --dest org.bluez --object-path "$path" \
          --method org.bluez.Device1.Disconnect >/dev/null 2>&1 \
          && ${notify} "Bluetooth" "Disconnected: $name" \
          || ${notify} -u critical "Bluetooth" "Failed to disconnect $name"
        ;;
      forget)
        mac="$2"; name="$3"; path=$(dev_path "$mac")
        ${gdbus} call --system --dest org.bluez --object-path ${adapter} \
          --method org.bluez.Adapter1.RemoveDevice "$path" >/dev/null 2>&1 \
          && ${notify} "Bluetooth" "Forgot: $name" \
          || ${notify} -u critical "Bluetooth" "Failed to forget $name"
        ;;
      power)
        [ "$2" = "on" ] && v=true || v=false
        ${gdbus} call --system --dest org.bluez --object-path ${adapter} \
          --method org.freedesktop.DBus.Properties.Set \
          org.bluez.Adapter1 Powered "<$v>" >/dev/null 2>&1 \
          && ${notify} "Bluetooth" "Powered $2" \
          || ${notify} -u critical "Bluetooth" "Failed to power $2"
        ;;
      *)
        echo "usage: btctl {pair|connect|disconnect|forget <mac> <name>|power on|off}" >&2
        exit 2
        ;;
    esac
  '';
in
{
  inherit btctl btscan;

  # Lua menu provider (see elephant.nix)
  menuLua = ''
    Name = "bluetooth"
    NamePretty = "Bluetooth"
    Icon = "bluetooth"
    Description = "Connect and pair Bluetooth devices"
    HideFromProviderlist = false
    SearchName = true
    FixedOrder = true
    -- Fallback action (entries provide their own via Actions table)
    Action = "sh -c '%VALUE%'"

    local GDBUS = "${gdbus}"
    local BTCTL = "${btctl}/bin/btctl"
    local ADAPTER = "${adapter}"

    -- Query D-Bus property, return as string
    local function prop(path, iface, name)
      local h = io.popen(GDBUS .. " call --system --dest org.bluez --object-path "
        .. path .. " --method org.freedesktop.DBus.Properties.Get "
        .. iface .. " " .. name .. " 2>/dev/null")
      if not h then return "" end
      local out = h:read("*a") or ""
      h:close()
      -- Strip gdbus output wrapping: "(<value>,)" and quotes
      local v = out:match("%(<(.-)>,?%)") or ""
      v = v:gsub("^'(.*)'$", "%1")
      v = v:gsub('^"(.*)"$', "%1")
      return v
    end

    -- List device paths via GetManagedObjects (avoids Introspect's escaped XML)
    local function list_devices()
      local paths, seen = {}, {}
      local h = io.popen(GDBUS .. " call --system --dest org.bluez --object-path /"
        .. " --method org.freedesktop.DBus.ObjectManager.GetManagedObjects 2>/dev/null")
      if h then
        local out = h:read("*a") or ""
        h:close()
        for node in out:gmatch("dev_[0-9A-Fa-f_]+") do
          if not seen[node] then
            seen[node] = true
            table.insert(paths, ADAPTER .. "/" .. node)
          end
        end
      end
      table.sort(paths)
      return paths
    end

    -- Format btctl arguments: <mac> "<name>" (drops quote-breaking chars)
    local function target(mac, name)
      return mac .. ' "' .. name:gsub('["$`\\\\]', "") .. '"'
    end

    -- Mode persisted in elephant's per-menu state
    local function in_scan_mode()
      local s = state()
      return s ~= nil and s[1] == "scan"
    end

    -- Enter scan mode and run discovery (AsyncClearReload re-queries after)
    function StartScan()
      setState({ "scan" })
      os.execute("${btscan}/bin/btscan 6")
    end

    -- Return to list mode
    function ShowPaired()
      setState({ "list" })
    end

    -- Pair device and return to list mode
    function Pair(value)
      os.execute(BTCTL .. " pair " .. value)
      setState({ "list" })
    end

    function GetEntries()
      local entries = {}

      -- Adapter off: single "power on" entry, reset to list mode
      if prop(ADAPTER, "org.bluez.Adapter1", "Powered") ~= "true" then
        setState({ "list" })
        table.insert(entries, {
          Text = "Bluetooth is off",
          Subtext = "power on",
          Value = "power on",
          Actions = { power_on = BTCTL .. " power on" },
        })
        return entries
      end

      local scanning = in_scan_mode()

      for _, path in ipairs(list_devices()) do
        local mac = path:match("dev_(.*)$"):gsub("_", ":")
        local name = prop(path, "org.bluez.Device1", "Alias")
        if name == "" then name = prop(path, "org.bluez.Device1", "Name") end
        if name == "" then name = mac end

        local paired = prop(path, "org.bluez.Device1", "Paired") == "true"
        local connected = prop(path, "org.bluez.Device1", "Connected") == "true"

        -- List: paired only | Scan: all devices with state markers
        if paired or scanning then
          local arg = target(mac, name)
          local actions = { scan = "lua:StartScan" }
          local subtext

          -- State shown in subtext only
          if connected then
            subtext = "connected"
            actions.disconnect = BTCTL .. " disconnect " .. arg
          elseif paired then
            subtext = "paired"
            actions.connect = BTCTL .. " connect " .. arg
          else
            subtext = "not paired"
            -- Lua action to return to list mode after pairing
            actions.pair = "lua:Pair"
          end

          if paired then
            actions.forget = BTCTL .. " forget " .. arg
          else
            local rssi = prop(path, "org.bluez.Device1", "RSSI")
            if rssi ~= "" then subtext = subtext .. " · rssi " .. rssi end
          end

          if scanning then
            actions.list = "lua:ShowPaired"
          else
            -- Hidden in scan mode to prevent power-off during pairing
            actions.power_off = BTCTL .. " power off"
          end

          table.insert(entries, {
            Text = name,
            Subtext = subtext,
            -- Argument for lua:Pair
            Value = arg,
            Actions = actions,
          })
        end
      end

      -- No devices: single scan action, reset to list mode
      if #entries == 0 then
        if scanning then setState({ "list" }) end
        table.insert(entries, {
          Text = scanning and "No devices found" or "No paired devices",
          Subtext = scanning and "put the device in pairing mode, then scan again"
            or "scan for nearby devices",
          Value = "scan",
          Actions = { scan = "lua:StartScan" },
        })
      end

      return entries
    end
  '';
}
