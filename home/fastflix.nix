{
  config,
  lib,
  pkgs,
  ...
}:

let
  theme = "system";
  portalizeQtPackage = import ./qt-portal-wrapper.nix { inherit pkgs; };

  fastflix = portalizeQtPackage (pkgs.fastflix.overrideAttrs (old: {
    postPatch = (old.postPatch or "") + ''
      substituteInPlace fastflix/models/config.py \
        --replace-fail 'theme: str = "onyx"' 'theme: str = "${theme}"'

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

  fastflixIcon =
    pkgs.runCommandLocal "fastflix-icon"
      {
        nativeBuildInputs = [ (pkgs.python3.withPackages (ps: [ ps.pillow ])) ];
      }
      ''
        mkdir -p "$out"
        python3 -c "from PIL import Image; Image.open('${pkgs.fastflix.src}/fastflix/data/icon.ico').convert('RGBA').resize((256, 256)).save('$out/fastflix.png')"
      '';
in
{
  home.packages = [ fastflix ];

  xdg.dataFile."icons/hicolor/256x256/apps/fastflix.png".source = "${fastflixIcon}/fastflix.png";

  xdg.desktopEntries.fastflix = {
    name = "FastFlix";
    genericName = "Video Encoder";
    comment = "Simple and friendly GUI for encoding videos";
    exec = lib.getExe fastflix;
    icon = "fastflix";
    terminal = false;
    categories = [
      "AudioVideo"
      "Video"
      "AudioVideoEditing"
    ];
  };

  home.activation.fastflixTheme = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    set -eu

    fastflixConfig="${config.xdg.dataHome}/FastFlix/fastflix.yaml"

    if [ -f "$fastflixConfig" ]; then
      $DRY_RUN_CMD ${pkgs.yq-go}/bin/yq -i '.theme = "${theme}"' "$fastflixConfig"
    fi
  '';
}
