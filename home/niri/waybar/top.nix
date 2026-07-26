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
    "custom/todo"
    "custom/media-prev"
    "custom/media"
    "custom/lyrics"
    "custom/media-next"
  ];
  modules-right = [
    "tray"
    "custom/cliphist"
    "custom/audio"
    "custom/bt"
    "custom/timer"
    "custom/powermenu"
  ];
}
// left
// center
// right
