{
  config,
  lib,
  pkgs,
  inputs,
  ...
}:
let
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

  fastflixTheme = "system";
  fastflixPackage = portalizeQtPackage (pkgs.fastflix.overrideAttrs (old: {
    postPatch = (old.postPatch or "") + ''
      substituteInPlace fastflix/models/config.py \
        --replace-fail 'theme: str = "onyx"' 'theme: str = "${fastflixTheme}"'

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

  _module.args.fastflixPackage = fastflixPackage;

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
    fastflixPackage

    inputs.sonora.packages.${pkgs.stdenv.hostPlatform.system}.default
  ];

  home.activation.fastflixTheme = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    set -eu
    fastflixConfig="${config.xdg.dataHome}/FastFlix/fastflix.yaml"
    if [ -f "$fastflixConfig" ]; then
      $DRY_RUN_CMD ${pkgs.yq-go}/bin/yq -i '.theme = "${fastflixTheme}"' "$fastflixConfig"
    fi
  '';
}
