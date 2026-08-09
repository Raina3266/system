{
  pkgs,
  ...
}:
{
  xdg.desktopEntries.rofi-theme-selector = {
    name = "Rofi Theme Selector";
    noDisplay = true;
    settings.Exec = "rofi-theme-selector";
    settings.Type = "Application";
  };
  
  xdg.portal = {
    enable = true;

    extraPortals = with pkgs; [
      xdg-desktop-portal-gtk
      xdg-desktop-portal-gnome
    ];

    config.niri = {
      default = [
        "gnome"
        "gtk"
      ];

      "org.freedesktop.impl.portal.FileChooser" = [
        "gtk"
      ];
    };
  };
  
  xdg.mimeApps = {
    enable = true;
    defaultApplications = {
      # File manager
      "inode/directory" = [ "nemo.desktop" ];

      # Terminal
      "x-scheme-handler/terminal" = [ "com.mitchellh.ghostty.desktop" ];

      # Web
      "text/html" = "google-chrome.desktop";
      "x-scheme-handler/http" = "google-chrome.desktop";
      "x-scheme-handler/https" = "google-chrome.desktop";
      "x-scheme-handler/about" = "google-chrome.desktop";
      "x-scheme-handler/unknown" = "google-chrome.desktop";

      # PDF
      "application/pdf" = [ "google-chrome.desktop" ];

      # Video
      "video/mp4" = [ "vlc.desktop" ];
      "video/x-matroska" = [ "vlc.desktop" ];
      "video/webm" = [ "vlc.desktop" ];
      "video/quicktime" = [ "vlc.desktop" ];
      "video/x-msvideo" = [ "vlc.desktop" ];
      "video/mpeg" = [ "vlc.desktop" ];
      "video/ogg" = [ "vlc.desktop" ];
      "video/3gpp" = [ "vlc.desktop" ];
      "video/3gpp2" = [ "vlc.desktop" ];
      "video/x-flv" = [ "vlc.desktop" ];
      "video/x-ms-wmv" = [ "vlc.desktop" ];
      "video/x-ms-asf" = [ "vlc.desktop" ];
      "video/divx" = [ "vlc.desktop" ];
      "video/mp2t" = [ "vlc.desktop" ];

      # Audio
      "audio/mpeg" = [ "vlc.desktop" ];
      "audio/mp4" = [ "vlc.desktop" ];
      "audio/x-m4a" = [ "vlc.desktop" ];
      "audio/ogg" = [ "vlc.desktop" ];
      "audio/flac" = [ "vlc.desktop" ];
      "audio/x-flac" = [ "vlc.desktop" ];
      "audio/wav" = [ "vlc.desktop" ];
      "audio/x-wav" = [ "vlc.desktop" ];
      "audio/webm" = [ "vlc.desktop" ];
      "audio/aac" = [ "vlc.desktop" ];
      "audio/x-aac" = [ "vlc.desktop" ];
      "audio/opus" = [ "vlc.desktop" ];
      "audio/x-matroska" = [ "vlc.desktop" ];
      "audio/x-ms-wma" = [ "vlc.desktop" ];
      "audio/vorbis" = [ "vlc.desktop" ];
      "audio/x-vorbis+ogg" = [ "vlc.desktop" ];
      "audio/ac3" = [ "vlc.desktop" ];
      "audio/eac3" = [ "vlc.desktop" ];
      "audio/x-ape" = [ "vlc.desktop" ];
      "audio/x-musepack" = [ "vlc.desktop" ];
      "audio/x-wavpack" = [ "vlc.desktop" ];
    };
  };
}
