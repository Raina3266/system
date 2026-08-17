{
  pkgs,
  config,
  lib,
  ...
}:
let
  themeDir = "${config.home.homeDirectory}/System/niri/themes";
  schemeName = "Bitpunk";
  schemeFile = "${config.home.homeDirectory}/.local/share/color-schemes/${schemeName}.colors";

  # The non-colour half of kdeglobals, so that a single activation step owns
  # the whole file rather than seeding it once and drifting afterwards.
  kdeglobalsExtra = pkgs.writeText "kdeglobals-extra" (
    lib.generators.toINI { } {
      General = {
        font = "Noto Sans,12";
        menuFont = "Noto Sans,11";
        toolBarFont = "Noto Sans,11";
        smallestReadableFont = "Noto Sans,11";
        fixed = "JetBrainsMono Nerd Font,12";
      };
    }
  );

  # A plain Qt6/KF6 dialog with no Plasma session dependency, so it works
  # under niri.  plasma-workspace is already in the closure for the menu file
  # below; link out this one binary instead of putting all of it on PATH.
  kcolorschemeeditor = pkgs.runCommand "kcolorschemeeditor" { } ''
    mkdir -p "$out/bin"
    test -e ${pkgs.kdePackages.plasma-workspace}/bin/kcolorschemeeditor
    ln -s ${pkgs.kdePackages.plasma-workspace}/bin/kcolorschemeeditor "$out/bin/"
  '';

  applyColors = pkgs.writeShellApplication {
    name = "kde-colors-apply";
    runtimeInputs = with pkgs; [
      coreutils
      python3
      dbus
    ];
    text = ''
      # A real file rather than a link: kcolorschemeeditor saves atomically and
      # would replace a symlink here with a regular file.
      install -Dm644 "${themeDir}/kde.colors" "${schemeFile}"

      python3 ${./merge-kdeglobals.py} "$HOME/.config/kdeglobals" \
        "${themeDir}/kde.colors" "${kdeglobalsExtra}"

      # KHintsSettings listens for this signal rather than watching the file,
      # so running Dolphin and Kid3 repaint without a restart.
      # PaletteChanged = 0.
      dbus-send --session --type=signal /KGlobalSettings \
        org.kde.KGlobalSettings.notifyChange int32:0 int32:0 || true
    '';
  };

  editColors = pkgs.writeShellApplication {
    name = "kde-colors-edit";
    runtimeInputs = [
      applyColors
      kcolorschemeeditor
      pkgs.coreutils
      pkgs.diffutils
    ];
    text = ''
      # kcolorschemeeditor can only ever save into ~/.local/share/color-schemes,
      # so drive it from the committed palette and fold the result back into
      # the repo afterwards.
      kde-colors-apply
      kcolorschemeeditor --overwrite ${schemeName} || true

      if cmp -s "${schemeFile}" "${themeDir}/kde.colors"; then
        echo "kde.colors unchanged"
      else
        cp "${schemeFile}" "${themeDir}/kde.colors"
        echo "updated ${themeDir}/kde.colors"
        kde-colors-apply
      fi
    '';
  };
in
{
  home.packages =
    (with pkgs; [
      kdePackages.dolphin
      kdePackages.breeze
      kdePackages.kio-fuse
      kdePackages.kfilemetadata
      kdePackages.kompare
      kdePackages.plasma-integration
    ])
    ++ [
      kcolorschemeeditor
      applyColors
      editColors
    ];

  # KColorScheme builds the palette from kdeglobals itself; a .colors file in
  # share/color-schemes is only consulted when kdeglobals has no [Colors:View]
  # group (KHintsSettings::loadPalettes).  Folding the palette in on every
  # activation is what makes editing niri/themes/kde.colors actually reach
  # Dolphin and Kid3, and it is also the only way [KDE] contrast and
  # frameContrast take effect, since Breeze reads those from kdeglobals.
  #
  # After linkGeneration so it is not undone by the file-link pass.
  home.activation.kdeColors = config.lib.dag.entryAfter [ "linkGeneration" ] ''
    $DRY_RUN_CMD ${applyColors}/bin/kde-colors-apply
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
