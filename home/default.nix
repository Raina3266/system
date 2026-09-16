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

  # The session-wide KDE platform theme opens an in-process file chooser that
  # renders but does not populate under PDF4QT. Keep the Kvantum application
  # style while sending only PDF4QT's file dialogs through the working portal.
  pdf4qtWithPortal = pkgs.symlinkJoin {
    name = "pdf4qt-${pkgs.pdf4qt.version}-portal";
    paths = [ pkgs.pdf4qt ];
    nativeBuildInputs = [ pkgs.makeWrapper ];
    postBuild = ''
      for program in "$out/bin/"*; do
        if [ -x "$program" ]; then
          wrapProgram "$program" --set QT_QPA_PLATFORMTHEME xdgdesktopportal
        fi
      done
    '';
  };
in
{
  imports = [
    ../niri
    ../themes
    ./shell
    ./cloud.nix
    ./desktop.nix
    ./fastflix.nix
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
    pdf4qtWithPortal
    obsidian
    krokiet
    exercism
    clash-verge-rev

    # Qt/Kde based.
    qdirstat
    qbittorrent
    kdePackages.kdenlive
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
    yt-dlp
    waylyrics

    inputs.sonora.packages.${pkgs.stdenv.hostPlatform.system}.default
  ];
}
