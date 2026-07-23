# Top-right waybar modules: clipboard, timer, bluetooth, audio sink,
# tray, power menu.
{ pkgs }:
let
  walker = "${pkgs.walker}/bin/walker";
  btctl = (import ../walker/bluetooth.nix { inherit pkgs; }).btctl;

  # Static launcher icons
  staticLauncher =
    name: icon: tooltip: walkerArgs:
    {
      format = "<span size='large'>${icon}</span>";
      return-type = "json";
      exec = pkgs.writeShellScript "waybar-${name}-poll" ''
        printf '{"text":"<span size='"'"'large'"'"'>${icon}</span>","tooltip":"${tooltip}"}'
      '';
      interval = 86400;
      on-click = pkgs.writeShellScript "waybar-${name}" ''
        ${walker} ${walkerArgs}
      '';
    };
in
{
  # Audio output launcher — icon + click opens the walker sink picker.
  # (Live sink name is shown in the walker's menu itself; volume lives
  # in the pulseaudio module in the system drawer.)
  "custom/audio-sink" = staticLauncher "audio-sink" "󰕾" "Audio output" "-m menus:audio-sink";

  "tray" = {
    icon-size = 18;
    spacing = 10;
  };

  "custom/cliphist" = staticLauncher "cliphist" "󰕛" "Clipboard history" "-m clipboard";

  "custom/bt" = {
    format = "{}";
    return-type = "json";
    exec = pkgs.writeShellScript "waybar-bt-poll" ''
      on_icon="<span size='large'>󰂯</span>"
      off_icon="<span size='large'>󰂱</span>"
      powered=$(bluetoothctl show 2>/dev/null | grep "Powered:" | awk '{print $2}')
      if [ "$powered" = "yes" ]; then
        names=$(bluetoothctl devices Connected 2>/dev/null | sed 's/^Device [0-9A-Fa-f:]* //' | tr '\n' ',' | sed 's/,$//')
        if [ -n "$names" ]; then
          printf '{"text":"%s","tooltip":"Connected: %s"}' "$on_icon" "$names"
        else
          printf '{"text":"%s","tooltip":"Bluetooth: On (no devices connected)"}' "$on_icon"
        fi
      else
        printf '{"text":"%s","tooltip":"Bluetooth: Off"}' "$off_icon"
      fi
    '';
    interval = 5;
    # Left-click: open the D-Bus bluetooth menu (walker menus:bluetooth).
    # Right-click: toggle power via D-Bus. Both go through btctl (see
    # walker/bluetooth.nix) — not bluetoothctl, whose one-shot agent
    # registration races the persistent bt-agent and breaks pairing.
    on-click = pkgs.writeShellScript "waybar-bt" ''
      ${walker} -m menus:bluetooth
    '';
    on-click-right = pkgs.writeShellScript "waybar-bt-toggle-power" ''
      powered=$(bluetoothctl show 2>/dev/null | grep "Powered:" | awk '{print $2}')
      if [ "$powered" = "yes" ]; then
        ${btctl}/bin/btctl power off
      else
        ${btctl}/bin/btctl power on
      fi
    '';
  };

  "custom/powermenu" = staticLauncher "powermenu" "󰐥" "Power menu" "-m menus:power";
}
// (import ./timer.nix { inherit pkgs; })
