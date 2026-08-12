{ pkgs }:
let
  # Keep this local package beside the Waybar module, matching the timer
  # package in top-right.nix and avoiding a separate package.nix.
  mediaControl = pkgs.rustPlatform.buildRustPackage {
    pname = "media-control";
    version = "0.1.0";

    src = ../../../scripts/media-control;
    cargoLock.lockFile = ../../../scripts/media-control/Cargo.lock;

    nativeBuildInputs = [ pkgs.makeWrapper ];

    postInstall = ''
      install -Dm644 ${../rofi/theme/media-control.rasi} \
        "$out/share/rofi/themes/media-control.rasi"

      wrapProgram "$out/bin/media-control" \
        --set MEDIA_CONTROL_PLAYERCTL "${pkgs.lib.getExe pkgs.playerctl}" \
        --set MEDIA_CONTROL_ROFI "${pkgs.lib.getExe pkgs.rofi}" \
        --set MEDIA_CONTROL_GDBUS "${pkgs.lib.getExe' pkgs.glib "gdbus"}" \
        --set MEDIA_CONTROL_FALLBACK_THEME "$out/share/rofi/themes/media-control.rasi"
    '';
  };
in
{
  inherit mediaControl;

  modules = {
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
  };
}
