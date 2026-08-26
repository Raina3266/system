{
  pkgs,
  lib,
  repoPackages,
  ...
}:
let
  # Listed once and used twice: as the Zed desktop entry's MimeType= line and
  # as the set of types that entry is the default handler for.
  codeMimeTypes = [
    "application/javascript"
    "application/json"
    "application/toml"
    "application/x-yaml"
    "application/xml"
    "text/css"
    "text/csv"
    "text/javascript"
    "text/markdown"
    "text/plain"
    "text/x-c++hdr"
    "text/x-c++src"
    "text/x-chdr"
    "text/x-csrc"
    "text/x-go"
    "text/x-log"
    "text/x-python"
    "text/x-rust"
    "text/x-shellscript"
    "text/x-yaml"
    "text/xml"
  ];

  webMimeTypes = [
    "application/pdf"
    "text/html"
    "x-scheme-handler/about"
    "x-scheme-handler/http"
    "x-scheme-handler/https"
    "x-scheme-handler/unknown"
  ];

  mediaMimeTypes = [
    "audio/aac"
    "audio/ac3"
    "audio/eac3"
    "audio/flac"
    "audio/mp4"
    "audio/mpeg"
    "audio/ogg"
    "audio/opus"
    "audio/vorbis"
    "audio/wav"
    "audio/webm"
    "audio/x-aac"
    "audio/x-ape"
    "audio/x-flac"
    "audio/x-m4a"
    "audio/x-matroska"
    "audio/x-musepack"
    "audio/x-ms-wma"
    "audio/x-vorbis+ogg"
    "audio/x-wav"
    "audio/x-wavpack"
    "video/3gpp"
    "video/3gpp2"
    "video/divx"
    "video/mp2t"
    "video/mp4"
    "video/mpeg"
    "video/ogg"
    "video/quicktime"
    "video/webm"
    "video/x-flv"
    "video/x-matroska"
    "video/x-ms-asf"
    "video/x-ms-wmv"
    "video/x-msvideo"
  ];

  handledBy = desktopEntry: types: lib.genAttrs types (_type: [ desktopEntry ]);
in
{
  home.packages =
    with pkgs;
    [
      kdePackages.dolphin
      kdePackages.ark
      kdePackages.baloo
      kdePackages.baloo-widgets
      kdePackages.kfilemetadata
      kdePackages.kio-fuse
      kdePackages.kompare
      kdePackages.dolphin-plugins
      kdePackages.plasma-integration
      kdePackages.systemsettings
      kdePackages.plasma-workspace
      kdePackages.knewstuff
    ]
    ++ [ repoPackages.ocrScreenshot ];

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
    mimeType = codeMimeTypes;
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

  dconf.settings = {
    "org/gnome/settings-daemon/plugins/media-keys" = {
      custom-keybindings = [
        "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/ocr-shortcut/"
      ];
    };
    "org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/ocr-shortcut" = {
      binding = "<Shift>Print";
      command = "${repoPackages.ocrScreenshot}/bin/ocr-screenshot";
      name = "OCR Screenshot";
    };
    "org/gnome/desktop/interface" = {
      enable-hot-corners = false;
      show-battery-percentage = true;
    };
    "org/gnome/mutter" = {
      center-new-windows = true;
    };
  };

  xdg.mimeApps = {
    enable = true;
    defaultApplications = {
      "x-scheme-handler/terminal" = [ "com.mitchellh.ghostty.desktop" ];
    }
    // handledBy "google-chrome.desktop" webMimeTypes
    // handledBy "zed-new-window.desktop" codeMimeTypes
    // handledBy "vlc.desktop" mediaMimeTypes;
  };

  systemd.user.services = {
    bt-agent = {
      Unit = {
        Description = "Persistent Bluetooth pairing agent";
        PartOf = [ "graphical-session.target" ];
        After = [ "graphical-session.target" ];
      };
      Service = {
        ExecStart = "${pkgs.bluez-tools}/bin/bt-agent --capability=DisplayYesNo";
        Restart = "on-failure";
      };
      Install.WantedBy = [ "graphical-session.target" ];
    };

    kde-baloo = {
      Unit = {
        Description = "Baloo File Indexer";
        PartOf = [ "graphical-session.target" ];
        After = [ "graphical-session.target" ];
      };
      Service = {
        ExecStart = "${pkgs.kdePackages.baloo}/libexec/kf6/baloo_file";
        Restart = "on-failure";
      };
      Install.WantedBy = [ "graphical-session.target" ];
    };
  };
}
