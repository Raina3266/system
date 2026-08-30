{
  config,
  pkgs,
  lib,
  repoPackages,
  ...
}:
let
  kwriteconfig = "${pkgs.kdePackages.kconfig}/bin/kwriteconfig6";
  kdeConfigHome = config.xdg.configHome;

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
  home.packages = [ repoPackages.ocrScreenshot ];

  xdg.configFile."menus/applications.menu".source =
    "${pkgs.kdePackages.plasma-workspace}/etc/xdg/menus/plasma-applications.menu";

  # kbd-layout-viewer5 ships from fcitx5-configtool, which is needed to
  # configure the input methods, so the package is kept and only the menu
  # entry is hidden. The binary stays callable from the terminal, and
  # `localectl status` / `setxkbmap -query` show the active layout.
  xdg.desktopEntries.kbd-layout-viewer5 = {
    name = "Keyboard layout viewer";
    noDisplay = true;
    settings.Exec = "kbd-layout-viewer5";
    settings.Type = "Application";
    settings.Icon = "input-keyboard";
    settings.Categories = "Qt;KDE;Utility;";
  };

  xdg.desktopEntries.zed-new-window = {
    name = "Zed (new window)";
    noDisplay = true;
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
      kdePackages.xdg-desktop-portal-kde
      xdg-desktop-portal-gtk
      xdg-desktop-portal-gnome
    ];

    # This file is selected only when XDG_CURRENT_DESKTOP is niri. GNOME
    # keeps its own portal preferences when that session is chosen in GDM.
    # Prefer KDE for visible desktop integration, while retaining the Niri-
    # compatible GNOME backends for screencasting and secret storage.
    config.niri = {
      default = [
        "kde"
        "gnome"
        "gtk"
      ];
      "org.freedesktop.impl.portal.FileChooser" = [
        "kde"
      ];
      "org.freedesktop.impl.portal.ScreenCast" = [
        "gnome"
      ];
      "org.freedesktop.impl.portal.Secret" = [
        "gnome-keyring"
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

  # Dolphin's "Open Terminal" (Shift+F4) goes through KIO's
  # KTerminalLauncherJob, which reads these two kdeglobals keys only: the
  # x-scheme-handler/terminal default above is never consulted, so without
  # them Dolphin falls back to Konsole. kdeglobals is mutable and already
  # written at activation by themes/default.nix, so the keys are set with
  # kwriteconfig rather than replacing the whole file.
  home.activation.setGhosttyAsKdeTerminal = config.lib.dag.entryAfter [ "linkGeneration" ] ''
    $DRY_RUN_CMD ${kwriteconfig} --file "${kdeConfigHome}/kdeglobals" \
      --group General --key TerminalApplication ghostty
    $DRY_RUN_CMD ${kwriteconfig} --file "${kdeConfigHome}/kdeglobals" \
      --group General --key TerminalService com.mitchellh.ghostty.desktop
  '';

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
