# Bluetooth D-Bus helpers, relocated out of the former walker/ integration so
# they survive independently. Used by the waybar bluetooth module.
#   btctl  — talk to BlueZ over D-Bus (gdbus) for pair/connect/disconnect/forget/
#            power. D-Bus is used instead of bluetoothctl to avoid pairing-agent
#            conflicts that would otherwise hang pairing; pairs with the
#            persistent bt-agent in ../../default.nix for auto-confirmation.
#   btscan — run a short discovery window (bluetoothctl --timeout keeps the
#            session alive long enough to populate the device list).
{ pkgs }:
let
  gdbus = "${pkgs.glib}/bin/gdbus";
  notify = "${pkgs.libnotify}/bin/notify-send";
  adapter = "/org/bluez/hci0";

  # btscan: run discovery for N seconds. bluetoothctl --timeout keeps session alive
  # (one-shot gdbus disconnects immediately and finds nothing). Temporary pairing
  # agent doesn't interfere since we never pair during scan.
  btscan = pkgs.writeShellScriptBin "btscan" ''
    secs="''${1:-6}"
    ${pkgs.bluez}/bin/bluetoothctl --timeout "$secs" scan on >/dev/null 2>&1
  '';

  # btctl: D-Bus command wrapper for menu actions (also used by waybar).
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
}