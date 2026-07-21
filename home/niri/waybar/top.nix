# Top waybar: clock, tray, hardware/system groups, media, utilities.
# Returns the topBar attrset (without bar outputs — merged by default.nix).
{ pkgs }:
let
  left = import ./top-left.nix { inherit pkgs; };
  center = import ./top-center.nix { inherit pkgs; };
  right = import ./top-right.nix { inherit pkgs; };
in
{
  layer = "top";
  position = "top";
  height = 40;
  smooth-scrolling-threshold = 5;
  expand-center = true;
  modules-left = [
    "custom/ycal"
    "group/system"
    "group/hardware"
  ];
  modules-center = [
    "custom/media-prev"
    "custom/media"
    "custom/lyrics"
    "custom/media-next"
    "custom/todo"
  ];
  modules-right = [
    "custom/cliphist"
    "custom/timer"
    "custom/bt"
    "custom/wifi"
    "custom/audio-sink"
    "tray"
    "custom/powermenu"
  ];
}
// left
// center
// right
