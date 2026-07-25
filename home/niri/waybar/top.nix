# Top bar: clock, tray, hardware/system, media, utilities
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
    "tray"
    "custom/cliphist"
    "custom/timer"
    "custom/audio"
    "custom/bt"
    "custom/powermenu"
  ];
}
// left
// center
// right
