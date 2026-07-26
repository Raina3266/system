# Top-right modules: clipboard, timer, bluetooth, audio, tray, power
{ pkgs }:
let
  walker = "${pkgs.walker}/bin/walker";
  btctl = (import ../walker/bluetooth.nix { inherit pkgs; }).btctl;

  # Static launcher icons with walker integration
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
  # Audio: opens walker device picker (volume in pulseaudio module)
  "custom/audio" = staticLauncher "audio" "󰕾" "Audio devices & volume" "-n -m menus:audio";

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
    # Left: open walker bluetooth menu | Right: toggle power
    # Uses btctl (D-Bus) to avoid bluetoothctl agent conflicts
    on-click = pkgs.writeShellScript "waybar-bt" ''
      ${walker} -n -m menus:bluetooth
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

  "custom/powermenu" = staticLauncher "powermenu" "󰐥" "Power menu" "-n -m menus:power";
}
// (import ./timer.nix { inherit pkgs; })
