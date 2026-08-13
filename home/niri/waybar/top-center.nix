{ pkgs }:
let
  # Package and Waybar integration stay together; only the Rasi theme is kept
  # separately so it remains easy to edit.
  mediaControl = pkgs.rustPlatform.buildRustPackage {
    pname = "media-control";
    version = "0.1.0";
    src = ../../../scripts/media-control;
    cargoLock.lockFile = ../../../scripts/media-control/Cargo.lock;

    nativeBuildInputs = [ pkgs.makeWrapper ];

    postInstall = ''
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

      # Left pauses every player; right opens the full controller.
      on-click = "${mediaControl}/bin/media-control pause-all";
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
