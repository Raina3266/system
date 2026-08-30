{
  pkgs,
  ...
}:
let
  krokiet = pkgs.runCommand "krokiet-${pkgs.czkawka-full.version}" { } ''
    cp -rL ${pkgs.czkawka-full} $out
    chmod -R +w $out
    rm -f $out/bin/czkawka_gui
    rm -f $out/share/applications/com.github.qarmin.czkawka.desktop
    rm -f $out/share/icons/hicolor/scalable/apps/com.github.qarmin.czkawka.svg
    rm -f $out/share/icons/hicolor/scalable/apps/com.github.qarmin.czkawka-symbolic.svg
    rm -f $out/share/metainfo/com.github.qarmin.czkawka.metainfo.xml
  '';
in
{
  imports = [
    ../niri
    ../themes
    ./shell
    ./yazi
    ./cloud.nix
    ./desktop.nix
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
    wechat
    qq
    whatsie
    telegram-desktop
    zoom-us
    wemeet
    
    # productivity / office
    obsidian
    krokiet
    exercism
    stirling-pdf-desktop

    # media playback
    lrcget

    # Qt/Kde based
    kdePackages.elisa
    kdePackages.dolphin
    kdePackages.ark
    kdePackages.baloo
    kdePackages.baloo-widgets
    kdePackages.kfilemetadata
    kdePackages.kio-fuse
    kdePackages.kompare
    kdePackages.dolphin-plugins
    kdePackages.plasma-integration
    kdePackages.knewstuff
    kdePackages.partitionmanagert
    vlc
    puddletag
    obs-studio
    shotcut
    kid3
    qbittorrent

    # media creation / editing
    pavucontrol
    inkscape
    gimp
    spotdl
    yt-dlp

    # downloads / torrent
    clash-verge-rev
  ];
}
