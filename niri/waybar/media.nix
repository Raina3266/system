# Media controls and browser MPRIS integration.
{ pkgs, packages }:
let
  mprisenceNativeHost =
    pkgs.writeTextDir "etc/chromium/native-messaging-hosts/mprisence.web.bridge.json"
      (builtins.toJSON {
        name = "mprisence.web.bridge";
        description = "Publish each browser media tab as an MPRIS player";
        path = "${pkgs.mprisence}/bin/mprisence";
        type = "stdio";
        allowed_origins = [
          "chrome-extension://pnkkjbdopihogobhhjbgapbpfccinjjo/"
          "chrome-extension://pphdmbejbipjlocngoefnmjoijcbdejf/"
        ];
      });
in
{
  homeConfig = {
    home.packages = [ pkgs.mprisence ];
    programs.google-chrome = {
      commandLineArgs = [
        "--disable-features=HardwareMediaKeyHandling,MediaSessionService"
      ];
      nativeMessagingHosts = [ mprisenceNativeHost ];
    };
  };

  modules = {
    "custom/media" = {
      hide-empty-text = true;
      format = "{}";
      return-type = "json";
      exec = "${packages.mediaControl}/bin/media-control waybar --watch --interval-ms 750";
      tooltip = true;
      escape = true;
      "restart-interval" = 2;
      "exec-on-event" = false;
      on-click = "${packages.mediaControl}/bin/media-control toggle";
      on-click-right = "${packages.mediaControl}/bin/media-control menu";
    };

    "custom/lyrics" = {
      hide-empty-text = true;
      return-type = "json";
      format = "󰝚  {text}";
      exec-if = "pgrep -x tauon >/dev/null || pgrep -x kid3 >/dev/null";
      exec = "${pkgs.waybar-lyric}/bin/waybar-lyric -qfpartial";
      on-click = "${pkgs.waybar-lyric}/bin/waybar-lyric play-pause";
    };
  };
}
