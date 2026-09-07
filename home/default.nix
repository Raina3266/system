{
  pkgs,
  inputs,
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
  music-python = python314.withPackages (ps: [
    ps.spotdl
  ]);
in
{
  imports = [
    ../niri
    ../themes
    ./shell
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
    # communication
    discord
    wechat
    qq
    whatsie
    telegram-desktop
    zoom-us
    wemeet
    handy
    wtype
    wl-clipboard

    # productivity
    digikam
    obsidian
    krokiet
    exercism
    clash-verge-rev

    # Qt/Kde based.
    qdirstat
    qbittorrent
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
    kdePackages.print-manager
    kdePackages.skanpage

    # media
    vlc
    puddletag
    obs-studio
    shotcut
    kid3
    gimp
    # Keep spotdl and its ytmusicapi dependency in the same Python 3.14
    # environment so music_organiser can invoke the matching python3.
    music-python
    yt-dlp
    waylyrics

    inputs.sonora.packages.${pkgs.system}.default
  ];
}
