# Bluetooth menu for walker/elephant, driven over the BlueZ D-Bus API
# (gdbus) instead of bluetoothctl.
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
#
# The menu has two modes, held in elephant's per-menu Lua state:
#
#   list (default) - paired devices only. Actions: connect/disconnect
#                    (default), forget, scan, power off.
#   scan           - everything bluetoothd currently knows, so freshly
#                    discovered devices can be paired. Already-paired
#                    devices stay visible, marked with their state.
#
# The mode sticks until changed, so typing to filter the list doesn't
# knock you out of scan mode. Pairing returns to the paired list (the
# point of scanning is done), as does the explicit "paired" action.
# Every action uses after = "AsyncClearReload" (see providers.nix), which
# re-queries once elephant reports the activation finished.
{ pkgs }:
let
  gdbus = "${pkgs.glib}/bin/gdbus";
  notify = "${pkgs.libnotify}/bin/notify-send";
  adapter = "/org/bluez/hci0";

  # btscan - run one scan session for N seconds. bluetoothd ties a
  # discovery session to the requesting D-Bus client and stops scanning
  # the moment that client disconnects, so one-shot gdbus StartDiscovery
  # calls (each client exits immediately) leave Discovering=false and
  # find nothing. bluetoothctl --timeout stays alive for the whole
  # window, which keeps the session alive. It temporarily registers its
  # pairing agent while running but unregisters on exit; we never pair
  # during a scan, so the persistent bt-agent is unaffected.
  #
  # writeShellScriptBin (not writeShellScript) so the store path is a
  # directory with bin/btscan - the menu Lua below calls
  # "${btscan}/bin/btscan", and writeShellScript would put the script
  # directly at ...-btscan (a file), making /bin/btscan "Not a
  # directory" and silently no-op'ing the action.
  btscan = pkgs.writeShellScriptBin "btscan" ''
    secs="''${1:-6}"
    ${pkgs.bluez}/bin/bluetoothctl --timeout "$secs" scan on >/dev/null 2>&1
  '';

  # btctl <verb> [args...] - small D-Bus helper used as the command for
  # every menu action (runs headless under elephant). Also handy from a
  # terminal, and used by waybar's right-click power toggle.
  # writeShellScriptBin for the same reason as btscan above.
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
        # "Already Exists" means already paired - treat as success.
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

  # Lua source for provider.menus.lua."bluetooth" (see elephant.nix).
  menuLua = ''
    Name = "bluetooth"
    NamePretty = "Bluetooth"
    Icon = "bluetooth"
    Description = "Connect and pair Bluetooth devices"
    HideFromProviderlist = false
    SearchName = true
    FixedOrder = true
    -- Unused: every entry supplies its command through its Actions
    -- table. Kept so an entry without actions would still do something.
    Action = "sh -c '%VALUE%'"

    local GDBUS = "${gdbus}"
    local BTCTL = "${btctl}/bin/btctl"
    local ADAPTER = "${adapter}"

    -- Query one property of one object; returns the value as a string.
    local function prop(path, iface, name)
      local h = io.popen(GDBUS .. " call --system --dest org.bluez --object-path "
        .. path .. " --method org.freedesktop.DBus.Properties.Get "
        .. iface .. " " .. name .. " 2>/dev/null")
      if not h then return "" end
      local out = h:read("*a") or ""
      h:close()
      -- gdbus prints "(<value>,)" - strip the wrapping punctuation and
      -- either quote style (gdbus uses '...' normally but "..." when
      -- the string itself contains a single quote, e.g. "Raina's").
      local v = out:match("%(<(.-)>,?%)") or ""
      v = v:gsub("^'(.*)'$", "%1")
      v = v:gsub('^"(.*)"$', "%1")
      return v
    end

    -- Device object paths known to the adapter. Uses GetManagedObjects
    -- (the ObjectManager at the root) rather than Introspect, whose XML
    -- gdbus returns with escaped quotes that are painful to match.
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

    -- Render "<mac> \"<name>\"" for use as btctl's arguments. Characters
    -- that would break out of the double quotes are dropped rather than
    -- escaped -- names are cosmetic here, the mac does the work.
    local function target(mac, name)
      return mac .. ' "' .. name:gsub('["$`\\\\]', "") .. '"'
    end

    -- Mode lives in elephant's per-menu state, which persists across
    -- the throwaway Lua states used for each query.
    local function in_scan_mode()
      local s = state()
      return s ~= nil and s[1] == "scan"
    end

    -- Bound to the "scan" action. Enters scan mode, then holds a
    -- discovery session open so nearby devices get a chance to show up.
    -- Walker's AsyncClearReload re-queries when this returns.
    function StartScan()
      setState({ "scan" })
      os.execute("${btscan}/bin/btscan 6")
    end

    -- Bound to the "paired" action: leave scan mode.
    function ShowPaired()
      setState({ "list" })
    end

    -- Bound to "pair": pair, then drop back to the paired list.
    function Pair(value)
      os.execute(BTCTL .. " pair " .. value)
      setState({ "list" })
    end

    function GetEntries()
      local entries = {}

      -- Powered off: exactly one entry, one action. Nothing else is
      -- actionable until the adapter is up. Also reset the mode so the
      -- menu comes back up as the paired list next time.
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

        -- List mode shows paired devices only; scan mode shows
        -- everything, with paired devices marked as such.
        if paired or scanning then
          local arg = target(mac, name)
          local actions = { scan = "lua:StartScan" }
          local subtext

          -- State is conveyed by the subtext alone; no glyph markers.
          if connected then
            subtext = "connected"
            actions.disconnect = BTCTL .. " disconnect " .. arg
          elseif paired then
            subtext = "paired"
            actions.connect = BTCTL .. " connect " .. arg
          else
            subtext = "not paired"
            -- Via Lua so it can drop back to the paired list on success;
            -- takes its arguments from Value below.
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
            -- Kept out of scan mode so nothing can power the adapter
            -- down midway through pairing.
            actions.power_off = BTCTL .. " power off"
          end

          table.insert(entries, {
            Text = name,
            Subtext = subtext,
            -- Passed to lua:Pair as its first argument.
            Value = arg,
            Actions = actions,
          })
        end
      end

      -- Nothing to show. Offer a single action so walker has an
      -- unambiguous default, and drop back to list mode so an empty
      -- scan can't leave the menu stuck in a state with no way out.
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
