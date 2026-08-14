{
  config,
  pkgs,
  ...
}:
{
  # Dolphin is a Qt/KDE application, so GTK CSS cannot theme it.  Use KDE's
  # native platform integration and Breeze widgets, then select the custom
  # colour scheme installed below.
  qt = {
    enable = true;
    platformTheme.name = "kde";
    style.name = "breeze";

    # Write mutable KConfig files during Home Manager activation.  Dolphin
    # remains free to persist unrelated preferences in the same files.
    kde.settings = {
      kdeglobals = {
        General.ColorScheme = "Cyberpunk";
        Icons.Theme = "breeze-dark";
      };

      dolphinrc = {
        General = {
          ShowFullPathInTitlebar = true;
          ShowStatusBar = "FullWidth";
          ShowToolTips = true;
          ShowZoomSlider = true;
        };
        DetailsMode.HighlightEntireRow = true;
      };
    };
  };

  home.packages = with pkgs.kdePackages; [
    dolphin
    dolphin-plugins
    ffmpegthumbs
    kdegraphics-thumbnailers
    kio-extras
  ];

  # Keep the palette editable alongside the other cyberpunk theme files.
  xdg.dataFile."color-schemes/Cyberpunk.colors".source =
    config.lib.file.mkOutOfStoreSymlink
      "/home/raina/System/home/niri/themes/Cyberpunk.colors";

  xdg.mimeApps.defaultApplications."inode/directory" = [
    "org.kde.dolphin.desktop"
  ];
}
