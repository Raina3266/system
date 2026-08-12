{ pkgs }:
let
  mediaControl = pkgs.callPackage ../../../scripts/media-control/package.nix { };
in
{
  "custom/media" = {
    hide-empty-text = true;
    format = "{}";
    return-type = "json";
    exec = "${mediaControl}/bin/media-control waybar --watch --interval-ms 750";
    tooltip = true;
    escape = true;
    "restart-interval" = 2;
    "exec-on-event" = false;

    # The requested left click pauses every currently playing MPRIS source.
    on-click = "${mediaControl}/bin/media-control pause-all";
    # Keep opening the controller available without changing left-click.
    on-click-right = "${mediaControl}/bin/media-control menu";
  };

  "custom/lyrics" = {
    hide-empty-text = true;
    return-type = "json";
    format = "{icon} {0}";
    format-icons = {
      playing = "󰝚 ";
      paused = "󰝚 ";
      lyric = "";
      music = "󰝚 ";
    };
    exec-if = "pgrep -x tauon >/dev/null || pgrep -x kid3 >/dev/null";
    exec = "${pkgs.waybar-lyric}/bin/waybar-lyric -qfpartial";
    on-click = "${pkgs.waybar-lyric}/bin/waybar-lyric play-pause";
  };
}

