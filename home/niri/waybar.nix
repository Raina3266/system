{
  pkgs,
  ...
}:
{
  # waybar-lyric is a standalone binary invoked by the custom/lyrics module below.
  home.packages = with pkgs; [
    waybar-lyric
    xwayland-satellite
  ];

  programs.waybar = {
    enable = true;

    settings = [
      {
        layer = "bottom";
        position = "top";
        height = 30;
        exclusive = true;
        gtk-layer-shell = true;
        passthrough = false;
        fixed-center = true;
        reload_style_on_change = true;
        margin = "4px 2px";

        modules-left = [
          "tray"
        ];
        modules-center = [
          "custom/lyrics"
        ];
        modules-right = [
          "cpu"
          "network"
          "clock"
        ];

        "custom/lyrics" = {
          return-type = "json";
          format = "{icon} {0}";
          hide-empty-text = true;
          format-icons = {
            playing = "▶";
            paused = "⏸";
            lyric = "🎵";
            music = "🎵";
          };
          exec-if = "which waybar-lyric";
          exec = "waybar-lyric -qfpartial";
          on-click = "waybar-lyric play-pause";
        };

        tray = {
          show-passive-items = true;
          spacing = 10;
        };

        clock = {
          format = "{:%H:%M}";
          tooltip = false;
        };

        network = {
          interval = 5;
          format-wifi = "📶 {essid}";
          format-ethernet = "🔗 {ipaddr}";
          format-disconnected = "⚠";
        };

        cpu = {
          format = "CPU {usage}%";
          tooltip = true;
          interval = 1;
        };
      }
    ];

    style = ''
      #custom-lyrics {
        color: #1db954;
        margin: 0 5px;
        padding: 0 10px;
      }

      #custom-lyrics.paused {
        color: #aaaaaa;
      }
    '';
  };
}
