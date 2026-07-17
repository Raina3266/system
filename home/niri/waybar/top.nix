# Top waybar: clock, tray, hardware/system groups, media, utilities.
# Returns the topBar attrset (without bar outputs — merged by default.nix).
{ pkgs, scripts }:
let
  left = import ./top-left.nix { inherit pkgs; };
  center = import ./top-center.nix { inherit pkgs; };
  right = import ./top-right.nix { inherit pkgs scripts; };
in
{
  layer = "top";
  position = "top";
  height = 36;
  smooth-scrolling-threshold = 5;

  modules-left = [
    "clock"
    "group/system"
    "group/hardware"
  ];
  modules-center = [
    "custom/media-prev"
    "custom/media"
    "custom/lyrics"
    "custom/media-next"
  ];
  modules-right = [
    "tray"
    "custom/cliphist"
    "custom/files"
    "custom/todo"
    "custom/timer"
    "custom/bt"
    "custom/wifi"
    "custom/audio-sink"
    "custom/powermenu"
  ];
} // left // center // right
