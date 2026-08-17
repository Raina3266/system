{
  pkgs,
  config,
  ...
}:
{
  home.packages = with pkgs; [
    kdePackages.dolphin
    kdePackages.breeze
    kdePackages.kio-fuse
    kdePackages.kfilemetadata
    kdePackages.kompare
    kdePackages.plasma-integration
    kdePackages.systemsettings
    kdePackages.plasma-workspace
    kdePackages.knewstuff
  ];

  home.activation.seedKdeglobals = config.lib.dag.entryAfter [ "writeBoundary" ] ''
      if [ ! -e "$HOME/.config/kdeglobals" ]; then
        $DRY_RUN_CMD cat > "$HOME/.config/kdeglobals" << 'EOF'
    [General]
    font=Noto Sans,12
    menuFont=Noto Sans,11
    toolBarFont=Noto Sans,11
    smallestReadableFont=Noto Sans,11
    fixed=JetBrainsMono Nerd Font,12
    EOF
      fi
  '';

  xdg.configFile."menus/applications.menu".source =
    "${pkgs.kdePackages.plasma-workspace}/etc/xdg/menus/plasma-applications.menu";

  xdg.desktopEntries.rofi-theme-selector = {
    name = "Rofi Theme Selector";
    noDisplay = true;
    settings.Exec = "rofi-theme-selector";
    settings.Type = "Application";
  };

  xdg.desktopEntries.zed-new-window = {
    name = "Zed (new window)";
    genericName = "Text Editor";
    exec = "zeditor -n %U";
    icon = "zed";
    terminal = false;
    categories = [
      "Utility"
      "TextEditor"
      "Development"
    ];
    mimeType = [
      "text/plain"
      "text/markdown"
      "text/x-python"
      "text/x-csrc"
      "text/x-chdr"
      "text/x-c++src"
      "text/x-c++hdr"
      "text/x-shellscript"
      "application/json"
      "application/x-yaml"
      "text/x-yaml"
      "application/toml"
      "text/x-rust"
      "text/x-go"
      "application/xml"
      "text/xml"
      "text/css"
      "application/javascript"
      "text/javascript"
      "text/x-log"
      "text/csv"
    ];
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
      # Text / code
      "text/plain" = [ "zed-new-window.desktop" ];
      "text/markdown" = [ "zed-new-window.desktop" ];
      "text/x-python" = [ "zed-new-window.desktop" ];
      "text/x-csrc" = [ "zed-new-window.desktop" ];
      "text/x-chdr" = [ "zed-new-window.desktop" ];
      "text/x-c++src" = [ "zed-new-window.desktop" ];
      "text/x-c++hdr" = [ "zed-new-window.desktop" ];
      "text/x-shellscript" = [ "zed-new-window.desktop" ];
      "application/json" = [ "zed-new-window.desktop" ];
      "application/x-yaml" = [ "zed-new-window.desktop" ];
      "text/x-yaml" = [ "zed-new-window.desktop" ];
      "application/toml" = [ "zed-new-window.desktop" ];
      "text/x-rust" = [ "zed-new-window.desktop" ];
      "text/x-go" = [ "zed-new-window.desktop" ];
      "application/xml" = [ "zed-new-window.desktop" ];
      "text/xml" = [ "zed-new-window.desktop" ];
      "text/css" = [ "zed-new-window.desktop" ];
      "application/javascript" = [ "zed-new-window.desktop" ];
      "text/javascript" = [ "zed-new-window.desktop" ];
      "text/x-log" = [ "zed-new-window.desktop" ];
      "text/csv" = [ "zed-new-window.desktop" ];
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
