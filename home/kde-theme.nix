{
  config,
  pkgs,
  ...
}:
let
  # Pin the upstream theme so a rebuild cannot silently change its appearance.
  daemonTheme = builtins.fetchGit {
    url = "https://github.com/MathisP75/daemon-kde-mk2.git";
    rev = "01bf4df4666e9021ac8013bc2c4eaabc8d312d68";
  };

  kwriteconfig = "${pkgs.kdePackages.kconfig}/bin/kwriteconfig6";
  kdeConfigHome = config.xdg.configHome;
in
{
  home.packages = with pkgs; [
    libsForQt5.qtstyleplugin-kvantum
    qt6Packages.qtstyleplugin-kvantum
  ];

  # Kvantum is the application style used by Daemon. The theme directory and
  # its selection file are both managed so System Settings cannot leave an old
  # Kvantum theme active.
  xdg.configFile = {
    "Kvantum/daemon-2.0" = {
      source = "${daemonTheme}/Kvantum/daemon-2.0";
    };
    "Kvantum/kvantum.kvconfig".text = ''
      [General]
      theme=daemon-2.0
    '';
  };

  # Install every KDE-facing component supplied by Daemon KDE MK2. Aurorae
  # and Plasma assets are harmless under niri and are ready if Plasma/KWin is
  # started later; Qt/KDE applications use the colours, icons and Kvantum now.
  xdg.dataFile = {
    "aurorae/themes/daemon-2.0" = {
      source = "${daemonTheme}/Window Decorations/daemon-2.0";
    };
    "color-schemes/Daemon2.colors".source = "${daemonTheme}/Color Scheme/Daemon2.colors";
    "icons/Daemon-Icons" = {
      source = "${daemonTheme}/Icon Theme/Daemon-Icons";
    };
    "konsole/Daemon-2.0.colorscheme".source = "${daemonTheme}/Konsole/Daemon-2.0.colorscheme";
    "plasma/desktoptheme/Daemon-2.0" = {
      source = "${daemonTheme}/Plasma Style/Daemon-2.0";
    };
    "plasma/look-and-feel/Daemon-2.0" = {
      source = "${daemonTheme}/Global Theme/Daemon-2.0";
    };
  };

  # Apply only appearance keys instead of replacing the complete KDE config;
  # Dolphin preferences and other unrelated KDE settings remain untouched.
  home.activation.applyDaemonKdeTheme = config.lib.dag.entryAfter [ "linkGeneration" ] ''
    $DRY_RUN_CMD ${pkgs.kdePackages.plasma-workspace}/bin/plasma-apply-colorscheme Daemon2

    $DRY_RUN_CMD ${kwriteconfig} --file "${kdeConfigHome}/kdeglobals" \
      --group General --key widgetStyle kvantum
    $DRY_RUN_CMD ${kwriteconfig} --file "${kdeConfigHome}/kdeglobals" \
      --group Icons --key Theme Daemon-Icons
    $DRY_RUN_CMD ${kwriteconfig} --file "${kdeConfigHome}/kdeglobals" \
      --group KDE --key LookAndFeelPackage Daemon-2.0

    $DRY_RUN_CMD ${kwriteconfig} --file "${kdeConfigHome}/plasmarc" \
      --group Theme --key name Daemon-2.0
    $DRY_RUN_CMD ${kwriteconfig} --file "${kdeConfigHome}/kwinrc" \
      --group org.kde.kdecoration2 --key library org.kde.kwin.aurorae
    $DRY_RUN_CMD ${kwriteconfig} --file "${kdeConfigHome}/kwinrc" \
      --group org.kde.kdecoration2 --key theme daemon-2.0
  '';
}
