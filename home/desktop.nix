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

  portalizeQtPackage =
    package:
    pkgs.symlinkJoin {
      name = "${package.name}-portal";
      paths = [ package ];
      nativeBuildInputs = [ pkgs.makeWrapper ];
      postBuild = ''
        for program in "$out/bin/"*; do
          if [ -f "$program" ] && [ -x "$program" ]; then
            wrapProgram "$program" --set QT_QPA_PLATFORMTHEME xdgdesktopportal
          fi
        done
      '';
      inherit (package) meta;
    };

  fastflixDefaultTheme = "system";
  fastflixPackage = portalizeQtPackage (pkgs.fastflix.overrideAttrs (old: {
    postPatch = (old.postPatch or "") + ''
      substituteInPlace fastflix/models/config.py \
        --replace-fail 'theme: str = "onyx"' 'theme: str = "${fastflixDefaultTheme}"'

      # Upstream passes argv lists through a shell, losing FFmpeg's arguments.
      for previewWindow in fastflix/widgets/windows/{crop_window,large_preview}.py; do
        substituteInPlace "$previewWindow" \
          --replace-fail 'run(thumb_command, shell=True, stderr=PIPE, stdout=PIPE)' \
            'run(thumb_command, stderr=PIPE, stdout=PIPE)'
      done

      # The stylesheet-free system theme still needs dark icons and text.
      substituteInPlace fastflix/resources.py \
        --replace-fail 'if theme.lower() in ("dark", "onyx"):' \
          'if theme.lower() in ("dark", "onyx", "system"):'
      substituteInPlace fastflix/widgets/main.py \
        --replace-fail 'self.app.fastflix.config.theme in ("dark", "onyx") else "color: black"' \
          'self.app.fastflix.config.theme in ("dark", "onyx", "system") else "color: black"'

      substituteInPlace fastflix/widgets/status_bar.py \
        --replace-fail '"#StatusBarWidget {  background-color: #f0f0f0;  border-top: 1px solid #cccccc;}"' '""' \
        --replace-fail '"color: #333333; background: transparent;"' '""'

      substituteInPlace fastflix/application.py \
        --replace-fail 'main_app.setApplicationDisplayName("FastFlix")' \
          'QtGui.QGuiApplication.setDesktopFileName("fastflix"); main_app.setApplicationDisplayName("FastFlix")'
    '';
  }));

  fastflixIcon =
    pkgs.runCommandLocal "fastflix-icon"
      {
        nativeBuildInputs = [ (pkgs.python3.withPackages (ps: [ ps.pillow ])) ];
      }
      ''
        mkdir -p "$out"
        python3 -c "from PIL import Image; Image.open('${pkgs.fastflix.src}/fastflix/data/icon.ico').convert('RGBA').resize((256, 256)).save('$out/fastflix.png')"
      '';

  krokiet = pkgs.runCommand "krokiet-${pkgs.czkawka-full.version}" { } ''
    cp -rL ${pkgs.czkawka-full} $out
    chmod -R +w $out
    rm -f $out/bin/czkawka_gui
    rm -f $out/share/applications/com.github.qarmin.czkawka.desktop
    rm -f $out/share/icons/hicolor/scalable/apps/com.github.qarmin.czkawka.svg
    rm -f $out/share/icons/hicolor/scalable/apps/com.github.qarmin.czkawka-symbolic.svg
    rm -f $out/share/metainfo/com.github.qarmin.czkawka.metainfo.xml
  '';

  pdf4qtWithPortal = portalizeQtPackage pkgs.pdf4qt;

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
  # Portals are configured system-wide in ../nixos/services.nix; declaring
  # them here as well would install a second copy into the user profile.

  home.packages = [
    repoPackages.ocrScreenshot
    fastflixPackage
    krokiet
    pdf4qtWithPortal
  ];

  xdg.configFile."menus/applications.menu".source =
    "${pkgs.kdePackages.plasma-workspace}/etc/xdg/menus/plasma-applications.menu";

  # ──────────────────────────────────────────────────────────────────────
  # GNOME Shell
  # ──────────────────────────────────────────────────────────────────────

  programs.gnome-shell = {
    enable = true;
    extensions = [
      { package = pkgs.gnomeExtensions.simple-timer; }
      { package = pkgs.gnomeExtensions.clipboard-history; }
      { package = pkgs.gnomeExtensions.astra-monitor; }
    ];
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

  # ──────────────────────────────────────────────────────────────────────
  # Desktop entries
  # ──────────────────────────────────────────────────────────────────────

  xdg.dataFile."icons/hicolor/256x256/apps/fastflix.png".source = "${fastflixIcon}/fastflix.png";

  xdg.desktopEntries.fastflix = {
    name = "FastFlix";
    genericName = "Video Encoder";
    comment = "Simple and friendly GUI for encoding videos";
    exec = lib.getExe fastflixPackage;
    icon = "fastflix";
    terminal = false;
    categories = [
      "AudioVideo"
      "Video"
      "AudioVideoEditing"
    ];
  };

  xdg.desktopEntries.btop = {
    name = "btop";
    exec = "ghostty -e btop";
    icon = "utilities-system-monitor";
    terminal = false;
  };

  # Rofi's wrapper prepends its package to XDG_DATA_DIRS. 
  xdg.dataFile."applications/rofi.desktop".text = ''
    [Desktop Entry]
    Type=Application
    Name=Rofi
    NoDisplay=true
    Exec=rofi -show
    Icon=rofi
    Terminal=false
  '';

  xdg.dataFile."applications/rofi-theme-selector.desktop".text = ''
    [Desktop Entry]
    Type=Application
    Name=Rofi Theme Selector
    NoDisplay=true
    Exec=rofi-theme-selector
    Icon=rofi
    Terminal=false
  '';

  xdg.desktopEntries.vim = {
    name = "Vim";
    noDisplay = true;
    exec = "vim %F";
    icon = "gvim";
    terminal = true;
    categories = [
      "Utility"
      "TextEditor"
    ];
  };

  xdg.desktopEntries.gvim = {
    name = "GVim";
    noDisplay = true;
    genericName = "Text Editor";
    exec = "gvim -f %F";
    icon = "gvim";
    terminal = false;
    categories = [
      "Utility"
      "TextEditor"
    ];
  };

  xdg.desktopEntries."org.fcitx.Fcitx5" = {
    name = "Fcitx 5";
    noDisplay = true;
    genericName = "Input Method";
    comment = "Start Input Method";
    exec = "fcitx5";
    icon = "fcitx";
    terminal = false;
    categories = [
      "System"
      "Utility"
    ];
    settings = {
      StartupNotify = "false";
      X-GNOME-AutoRestart = "false";
      X-GNOME-Autostart-Notify = "false";
      X-KDE-autostart-after = "panel";
      X-KDE-StartupNotify = "false";
      X-KDE-Wayland-VirtualKeyboard = "true";
      X-KDE-Wayland-Interfaces = "org_kde_plasma_window_management";
    };
  };

  xdg.desktopEntries.fcitx5-configtool = {
    name = "Fcitx 5 Configuration";
    noDisplay = true;
    genericName = "Input Method Configuration";
    comment = "Change Fcitx 5 Configuration";
    exec = "fcitx5-configtool";
    icon = "fcitx";
    terminal = false;
    categories = [ "Settings" ];
  };

  xdg.desktopEntries."org.fcitx.fcitx5-migrator" = {
    name = "Fcitx 5 Migration Wizard";
    noDisplay = true;
    comment = "Import data from other input method such as Fcitx 4";
    exec = "fcitx5-migrator";
    icon = "fcitx";
    terminal = false;
    categories = [ "Settings" ];
  };

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

  xdg.desktopEntries.qv4l2 = {
    name = "Qt V4L2 test Utility";
    noDisplay = true;
    exec = "qv4l2";
    icon = "qv4l2";
    terminal = false;
    categories = [ "AudioVideo" ];
  };

  xdg.desktopEntries.qvidcap = {
    name = "Qt V4L2 video capture utility";
    noDisplay = true;
    exec = "qvidcap";
    icon = "qvidcap";
    terminal = false;
    categories = [ "AudioVideo" ];
  };

  xdg.desktopEntries.cups = {
    name = "Manage Printing";
    noDisplay = true;
    exec = "xdg-open http://localhost:631/";
    icon = "cups";
    terminal = false;
    categories = [
      "System"
      "Settings"
      "Printing"
      "HardwareSettings"
      "X-Red-Hat-Base"
    ];
  };

  xdg.desktopEntries.nvim = {
    name = "Neovim wrapper";
    noDisplay = true;
    genericName = "Text Editor";
    exec = "nvim %F";
    icon = "nvim";
    terminal = true;
    categories = [
      "Utility"
      "TextEditor"
      "Development"
    ];
  };

  xdg.desktopEntries.nixos-manual = {
    name = "NixOS Manual";
    noDisplay = true;
    genericName = "System Manual";
    exec = "nixos-help";
    icon = "nix-snowflake";
    terminal = false;
    categories = [ "System" ];
  };

  xdg.desktopEntries."org.kde.ark" = {
    name = "Ark";
    noDisplay = true;
    genericName = "Archiving Tool";
    exec = "ark %U";
    icon = "ark";
    terminal = false;
    categories = [
      "Qt"
      "KDE"
      "Utility"
    ];
  };

  xdg.desktopEntries."com.ulduzsoft.Birdtray" = {
    name = "Birdtray";
    noDisplay = true;
    exec = "birdtray";
    icon = "com.ulduzsoft.Birdtray";
    terminal = false;
    categories = [
      "Email"
    ];
  };
}
