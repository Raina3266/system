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
  music-python = pkgs.runCommand "music-python" { } ''
    mkdir -p $out/bin
    ln -s ${pkgs.python3.withPackages (ps: [ ps.ytmusicapi ])}/bin/python3 $out/bin/python3
  '';
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
    # Expose only the Python interpreter so it does not conflict with
    # spotdl's own ytmusicapi wrapper in the Home Manager build environment.
    spotdl
    music-python
    yt-dlp
    waylyrics

    inputs.sonora.packages.${pkgs.system}.default
  ];
}
