# Bluetooth menu for walker/elephant, driven entirely over the BlueZ
# D-Bus API (gdbus) instead of bluetoothctl.
#
# Why not bluetoothctl: elephant's built-in bluetooth provider feeds
# "pair <mac>\nquit" into a one-shot bluetoothctl, which exits before
# the pairing exchange finishes. Worse, every one-shot bluetoothctl
# registers itself as the default pairing agent and unregisters on
# exit, racing the persistent bt-agent service (../default.nix) and
# leaving windows with NO default agent -> pair hangs at "Pairing...".
#
# Talking to bluetoothd directly via D-Bus needs no agent inside this
# process at all: bluetoothd forwards confirmation requests to the
# persistent bt-agent, which auto-accepts (DisplayYesNo).
{ pkgs }:
let
  gdbus = "${pkgs.glib}/bin/gdbus";
  notify = "${pkgs.libnotify}/bin/notify-send";

  # btscan — run one scan session for N seconds. bluetoothd ties a
  # discovery session to the requesting D-Bus client and stops
  # scanning the moment that client disconnects, so one-shot gdbus
  # StartDiscovery calls (each client exits immediately) leave
  # Discovering=false and find nothing. bluetoothctl --timeout stays
  # alive for the whole window, which keeps the session alive.
  # It temporarily registers its pairing agent while running, but
  # unregisters on exit; we never pair during a scan, so the
  # persistent bt-agent is unaffected.
  #
  # writeShellScriptBin (not writeShellScript) so the store path is
  # a directory with bin/btscan — the menu Lua below calls
  # "${btscan}/bin/btscan", and writeShellScript would put the script
  # directly at ...-btscan (a file), making /bin/btscan "Not a directory"
  # and silently no-op'ing every bluetooth menu action.
  btscan = pkgs.writeShellScriptBin "btscan" ''
    secs="''${1:-4}"
    ${pkgs.bluez}/bin/bluetoothctl --timeout "$secs" scan on >/dev/null 2>&1
  '';

  # btctl <verb> [args...] — small D-Bus helper used as the Value of
  # every menu entry (runs headless under elephant).
  # writeShellScriptBin for the same reason as btscan above: the Lua
  # calls "${btctl}/bin/btctl ...".
  btctl = pkgs.writeShellScriptBin "btctl" ''
    set -u
    dev_path() { printf '/org/bluez/hci0/dev_%s' "$(printf '%s' "$1" | tr ':' '_')"; }

    case "$1" in
      pair)
        # Pair, then trust so the device auto-reconnects, then connect.
        # Pair takes a while; give notifications a title only at the end.
        mac="$2"; name="$3"; path=$(dev_path "$mac")
        err=$(${gdbus} call --system --dest org.bluez --object-path "$path" \
              --method org.bluez.Device1.Pair 2>&1)
        rc=$?
        # "Already Exists" is fine — treat as success.
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
        ${gdbus} call --system --dest org.bluez --object-path /org/bluez/hci0 \
          --method org.bluez.Adapter1.RemoveDevice "$path" >/dev/null 2>&1 \
          && ${notify} "Bluetooth" "Forgot: $name" \
          || ${notify} -u critical "Bluetooth" "Failed to forget $name"
        ;;
      power)
        # power on|off — also useful from a terminal.
        [ "$2" = "on" ] && v=true || v=false
        ${gdbus} call --system --dest org.bluez --object-path /org/bluez/hci0 \
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

  # Lua source for provider.menus.lua."bluetooth" (see elephant.nix).
  # Lists all devices bluetoothd knows (paired + discovered), with the
  # default action doing the sensible thing per state:
  #   connected  -> disconnect
  #   paired     -> connect
  #   unpaired   -> pair
  # Extra actions: ctrl+f forget (paired only), ctrl+r rescan.
  menuLua = ''
    Name = "bluetooth"
    NamePretty = "Bluetooth"
    Icon = "bluetooth"
    Action = "sh -c '%VALUE%'"
    HideFromProviderlist = false
    Description = "Pair / connect Bluetooth devices"
    SearchName = true

    local GDBUS = "${gdbus}"

    -- Query one property of one object; returns the raw value string.
    local function prop(path, iface, name)
      local h = io.popen(GDBUS .. " call --system --dest org.bluez --object-path "
        .. path .. " --method org.freedesktop.DBus.Properties.Get "
        .. iface .. " " .. name .. " 2>/dev/null")
      if not h then return "" end
      local out = h:read("*a") or ""
      h:close()
      -- gdbus prints "(<value>,)" — strip wrapping punctuation and
      -- either quote style (gdbus uses '...' normally but "..." when
      -- the string itself contains a single quote, e.g. "Raina's").
      local v = out:match("%(<(.-)>,?%)") or ""
      v = v:gsub("^'(.*)'$", "%1")
      v = v:gsub('^"(.*)"$', "%1")
      return v
    end

    -- List device object paths known to the adapter. Uses
    -- GetManagedObjects (the ObjectManager at the root) rather than
    -- Introspect, whose XML gdbus returns with escaped quotes that are
    -- painful to pattern-match. Output contains object paths like
    -- objectpath '/org/bluez/hci0/dev_AA_BB_...' — we extract those.
    local function list_devices()
      local paths = {}
      local h = io.popen(GDBUS .. " call --system --dest org.bluez --object-path /"
        .. " --method org.freedesktop.DBus.ObjectManager.GetManagedObjects 2>/dev/null")
      if h then
        local out = h:read("*a") or ""
        h:close()
        for node in out:gmatch("/org/bluez/hci0/(dev_[0-9A-Fa-f_]+)") do
          local path = "/org/bluez/hci0/" .. node
          local dup = false
          for _, p in ipairs(paths) do if p == path then dup = true break end end
          if not dup then table.insert(paths, path) end
        end
      end
      return paths
    end

    function GetEntries()
      local entries = {}

      local powered = prop("/org/bluez/hci0", "org.bluez.Adapter1", "Powered")
      if powered ~= "true" then
        table.insert(entries, {
          Text = "󰂲  Bluetooth is off — Power On",
          Value = "${btctl}/bin/btctl power on",
          Actions = { rescan = "${btscan}/bin/btscan 4" },
        })
        return entries
      end

      -- Scan briefly so devices in pairing mode show up on first open.
      -- btscan holds a D-Bus client alive for the whole window;
      -- plain gdbus StartDiscovery would stop instantly (see btscan).
      os.execute("${btscan}/bin/btscan 4")

      local devices = list_devices()
      table.sort(devices)

      for _, path in ipairs(devices) do
        local mac = path:match("dev_(.*)$"):gsub("_", ":")
        local name = prop(path, "org.bluez.Device1", "Alias")
        if name == "" then name = prop(path, "org.bluez.Device1", "Name") end
        if name == "" then name = mac end
        local paired = prop(path, "org.bluez.Device1", "Paired") == "true"
        local connected = prop(path, "org.bluez.Device1", "Connected") == "true"
        local rssi = prop(path, "org.bluez.Device1", "RSSI")

        local marker, state, default_verb
        if connected then
          marker, state, default_verb = "✓", "connected", "disconnect"
        elseif paired then
          marker, state, default_verb = "●", "paired", "connect"
        else
          marker, state, default_verb = "○", "new", "pair"
        end

        local subtext = state
        if rssi ~= "" then subtext = subtext .. " · rssi " .. rssi end

        -- Per-entry actions, shown as clickable hint buttons (bottom
        -- right) when the entry is selected. Labels come from
        -- actions.nix (forget / rescan / power_off).
        local safe_name = name:gsub('[\"]', "")
        local actions = {
          rescan = "${btscan}/bin/btscan 4",
          power_off = "${btctl}/bin/btctl power off",
        }
        if paired then
          actions.forget = "${btctl}/bin/btctl forget " .. mac .. " \"" .. safe_name .. "\""
        end

        table.insert(entries, {
          Text = marker .. "  " .. name,
          Subtext = subtext,
          Value = "${btctl}/bin/btctl " .. default_verb .. " " .. mac
            .. " \"" .. safe_name .. "\"",
          Actions = actions,
        })
      end

      if #entries == 0 then
        table.insert(entries, {
          Text = "No devices found",
          Subtext = "Put the device in pairing mode, then press ctrl+r",
          Value = "true",
          Actions = {
            rescan = "${btscan}/bin/btscan 4",
            power_off = "${btctl}/bin/btctl power off",
          },
        })
      end

      return entries
    end
  '';
}
