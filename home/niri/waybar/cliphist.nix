# Clipboard history (cliphist) for Wayland, wired up with rofi.
{ pkgs }:
let
  cliphist = "${pkgs.cliphist}/bin/cliphist";
  rofi = "${pkgs.rofi}/bin/rofi";
  wlCopy = "${pkgs.wl-clipboard}/bin/wl-copy";

  # Poll script: static icon with tooltip
  cliphistPoll = pkgs.writeShellScript "waybar-cliphist-poll" ''
    printf '{"tooltip":"Clipboard history"}'
  '';

  # Picker: list history → rofi dmenu → decode → copy
  cliphistPick = pkgs.writeShellScript "waybar-cliphist" ''
    ${cliphist} list \
      | ${rofi} -dmenu -i -p "clipboard" \
          -display-column-separator '\t' -display-columns 2 \
          -theme "~/.config/rofi/rofi-single.rasi" \
          -theme-str 'window {width: 500px;}' \
      | ${cliphist} decode \
      | ${wlCopy}
  '';
in
{
  "custom/cliphist" = {
    format = "<span size='large'>󰕛 </span>";
    return-type = "json";
    exec = cliphistPoll;
    interval = 86400;
    on-click = cliphistPick;
  };
}
