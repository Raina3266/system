{
  config,
  lib,
  pkgs,
  ...
}:

let
  # Which of FastFlix's four themes to run. "onyx", "dark" and "light" each
  # paint a bundled stylesheet over Qt; "system" is the one that paints none,
  # so it is what hands the window to Kvantum and the Daemon palette that
  # ../themes/default.nix configures.
  theme = "system";

  # Nixpkgs packages FastFlix exactly as upstream ships it: no desktop entry,
  # no icon outside a Windows .ico, and a bundled Breeze stylesheet painted
  # over whatever Qt style is configured.
  fastflix = pkgs.fastflix.overrideAttrs (old: {
    # The session-wide KDE platform theme opens its file chooser in-process.
    # With FastFlix's PySide6 wrapper and the mixed Qt 5/6 session plugin path,
    # that chooser renders but never populates its file view.  Use the portal
    # implementation for this app only; the KDE portal still supplies the
    # native chooser, while Kvantum continues to style the FastFlix window.
    makeWrapperArgs = (old.makeWrapperArgs or [ ]) ++ [
      "--set"
      "QT_QPA_PLATFORMTHEME"
      "xdgdesktopportal"
    ];

    postPatch = (old.postPatch or "") + ''
      substituteInPlace fastflix/models/config.py \
        --replace-fail 'theme: str = "onyx"' 'theme: str = "${theme}"'

      # These commands are already argv lists. On POSIX, combining a list with
      # shell=True runs only argv[0] through the shell, so FFmpeg receives no
      # options and prints its usage instead of producing a preview image.
      for previewWindow in fastflix/widgets/windows/{crop_window,large_preview}.py; do
        substituteInPlace "$previewWindow" \
          --replace-fail 'run(thumb_command, shell=True, stderr=PIPE, stdout=PIPE)' \
            'run(thumb_command, stderr=PIPE, stdout=PIPE)'
      done

      # FastFlix picks its icon set and its hard-coded label colours from that
      # same name and only recognises its own two dark themes. Daemon is dark
      # as well, so count "system" as dark instead of drawing black on it.
      substituteInPlace fastflix/resources.py \
        --replace-fail 'if theme.lower() in ("dark", "onyx"):' \
          'if theme.lower() in ("dark", "onyx", "system"):'
      substituteInPlace fastflix/widgets/main.py \
        --replace-fail 'self.app.fastflix.config.theme in ("dark", "onyx") else "color: black"' \
          'self.app.fastflix.config.theme in ("dark", "onyx", "system") else "color: black"'

      # Same for the one widget FastFlix paints light by default: an empty
      # stylesheet hands the status bar back to the configured Qt style.
      substituteInPlace fastflix/widgets/status_bar.py \
        --replace-fail '"#StatusBarWidget {  background-color: #f0f0f0;  border-top: 1px solid #cccccc;}"' '""' \
        --replace-fail '"color: #333333; background: transparent;"' '""'

      # Qt falls back to the interpreter's file name for the Wayland app ID,
      # which ties the window to neither the desktop entry below nor a niri
      # window rule.
      substituteInPlace fastflix/application.py \
        --replace-fail 'main_app.setApplicationDisplayName("FastFlix")' \
          'QtGui.QGuiApplication.setDesktopFileName("fastflix"); main_app.setApplicationDisplayName("FastFlix")'
    '';
  });

  # The icon theme specification has no directory for .ico, the only
  # application icon in the source. Convert its largest frame to a PNG at the
  # size of the hicolor directory it is linked into below.
  fastflixIcon =
    pkgs.runCommandLocal "fastflix-icon"
      {
        nativeBuildInputs = [ (pkgs.python3.withPackages (ps: [ ps.pillow ])) ];
      }
      ''
        mkdir -p "$out"
        python3 -c "from PIL import Image; Image.open('${fastflix.src}/fastflix/data/icon.ico').convert('RGBA').resize((256, 256)).save('$out/fastflix.png')"
      '';
in
{
  home.packages = [ fastflix ];

  xdg.dataFile."icons/hicolor/256x256/apps/fastflix.png".source = "${fastflixIcon}/fastflix.png";

  # Upstream ships no desktop entry at all, so the encoder was reachable only
  # from a shell. Run it by store path: the launcher then always starts this
  # build rather than whatever else put a fastflix on PATH.
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

  # FastFlix rewrites its whole config on every run, so the file cannot be a
  # store symlink. Set the one key that matters in place, the way
  # ../themes/default.nix applies KDE's appearance keys; a config that does
  # not exist yet gets the patched default instead.
  home.activation.fastflixTheme = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    set -eu

    fastflixConfig="${config.xdg.dataHome}/FastFlix/fastflix.yaml"

    if [ -f "$fastflixConfig" ]; then
      $DRY_RUN_CMD ${pkgs.yq-go}/bin/yq -i '.theme = "${theme}"' "$fastflixConfig"
    fi
  '';
}
