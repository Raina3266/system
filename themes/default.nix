# Every theme this configuration applies, in one module.
#
# Two mechanisms live here. The stylesheets in this repository are linked into
# place with mkOutOfStoreSymlink, so they stay outside the Nix store and an
# edit is picked up the next time the program starts without a Home Manager
# rebuild. The Daemon KDE MK2 theme comes from a pinned upstream checkout and
# is copied in from the store, plus the activation step that selects it.
{
  config,
  pkgs,
  ...
}:
let
  # --- Repository stylesheets, linked out of the store -----------------------

  repoRoot = "${config.home.homeDirectory}/System";

  link = path: { source = config.lib.file.mkOutOfStoreSymlink "${repoRoot}/${path}"; };

  # <path under $XDG_CONFIG_HOME> = <path in this repository>
  configLinks = {
    "gtk-3.0/gtk.css" = "themes/gtk3.css"; # GTK3 apps (pavucontrol, file dialogs)
    "gtk-4.0/gtk.css" = "themes/gtk4.css"; # GTK4 apps (portal file chooser, image viewer)
    "preview-panel/preview-panel.css" = "themes/preview-panel.css";
    "rofi/config.rasi" = "niri/rofi/config.rasi";
    "rofi/media-control.rasi" = "themes/media-control.rasi";
    "rofi/rofi-audio.rasi" = "themes/rofi-audio.rasi";
    "rofi/rofi-clipboard.rasi" = "themes/rofi-clipboard.rasi";
    "rofi/rofi-finder.rasi" = "themes/rofi-finder.rasi";
    "rofi/rofi-network.rasi" = "themes/rofi-network.rasi";
    "waybar/style.css" = "themes/waybar.css";
    "yazi/theme.toml" = "themes/yazi.toml";
  };

  # <path under $XDG_DATA_HOME> = <path in this repository>
  dataLinks = {
    "fcitx5/themes/cyberpunk/theme.conf" = "themes/fcitx5.conf";
  };

  # --- Daemon KDE MK2, from upstream ----------------------------------------

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
  xdg.configFile = (builtins.mapAttrs (_name: link) configLinks) // {
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
  xdg.dataFile = (builtins.mapAttrs (_name: link) dataLinks) // {
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
      --group KDE --key widgetStyle kvantum
    $DRY_RUN_CMD ${kwriteconfig} --file "${kdeConfigHome}/kdeglobals" \
      --group Icons --key Theme Daemon-Icons
    $DRY_RUN_CMD ${kwriteconfig} --file "${kdeConfigHome}/kdeglobals" \
      --group KDE --key LookAndFeelPackage Daemon-2.0

    $DRY_RUN_CMD ${kwriteconfig} --file "${kdeConfigHome}/plasmarc" \
      --group Theme --key name Daemon-2.0
    $DRY_RUN_CMD ${kwriteconfig} --file "${kdeConfigHome}/kwinrc" \
      --group org.kde.kdecoration2 --key library org.kde.kwin.aurorae
    $DRY_RUN_CMD ${kwriteconfig} --file "${kdeConfigHome}/kwinrc" \
      --group org.kde.kdecoration2 --key theme __aurorae__svg__daemon-2.0
  '';
}
