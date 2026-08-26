{
  pkgs,
  config,
  ...
}:
let
  repoRoot = "${config.home.homeDirectory}/System";

  link = path: { source = config.lib.file.mkOutOfStoreSymlink "${repoRoot}/${path}"; };

  configLinks = {
    "gtk-3.0/gtk.css" = "themes/gtk3.css";
    "gtk-4.0/gtk.css" = "themes/gtk4.css";
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

  dataLinks = {
    "fcitx5/themes/cyberpunk/theme.conf" = "themes/fcitx5.conf";
  };
in
{
  imports = [
    ../niri
    ./shell
    ./yazi
    ./cloud.nix
    ./desktop.nix
    ./kde-theme.nix
    ./office.nix
  ];

  home = {
    username = "raina";
    homeDirectory = "/home/raina";
    stateVersion = "26.05";
  };

  programs = {
    home-manager.enable = true;
    zed-editor.enable = true;
    vscode.enable = true;
    google-chrome.enable = true;
    firefox.enable = true;
  };

  home.packages = with pkgs; [
    # handy
    handy
    wtype
    wl-clipboard

    # communication
    discord
    zoom-us
    wechat
    qq
    wemeet
    whatsie
    telegram-desktop

    # productivity / office
    obsidian
    meld
    czkawka
    exercism
    stirling-pdf-desktop

    # media playback
    lrcget
    vlc
    puddletag
    waylyrics
    kdePackages.elisa

    # media creation / editing
    pavucontrol
    obs-studio
    inkscape
    gimp
    shotcut
    kid3
    spotdl
    yt-dlp

    # downloads / torrent
    qbittorrent
    clash-verge-rev

    # can be removed when it update later
    (tauon.overrideAttrs (old: {
      makeWrapperArgs = (old.makeWrapperArgs or [ ]) ++ [
        "--prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath [ libappindicator ]}"
      ];
    }))
  ];

  xdg.configFile = builtins.mapAttrs (_name: link) configLinks;
  xdg.dataFile = builtins.mapAttrs (_name: link) dataLinks;
}
