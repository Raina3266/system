# Packages built from this repository and desktop integrations used by modules.
{
  pkgs,
  kernelPackages ? pkgs.linuxPackages_latest,
}:
let
  mkWorkspacePackage = pname: extra:
    pkgs.rustPlatform.buildRustPackage (
      {
        inherit pname;
        version = "0.1.0";
        src = ./.;
        cargoLock.lockFile = ./Cargo.lock;
        cargoBuildFlags = [
          "--package"
          pname
        ];
        cargoTestFlags = [
          "--package"
          pname
        ];
      }
      // extra
    );

  withParentDeath = pkgs.runCommandCC "with-parent-death" {
    src = pkgs.writeText "with-parent-death.c" ''
      #include <sys/prctl.h>
      #include <unistd.h>
      #include <signal.h>
      #include <stdlib.h>
      #include <stdio.h>

      int main(int argc, char *argv[]) {
        if (argc < 2) {
          fprintf(stderr, "usage: with-parent-death PROGRAM [ARGS...]\n");
          return 2;
        }
        prctl(PR_SET_PDEATHSIG, SIGKILL);
        if (getppid() == 1) _exit(0);
        execvp(argv[1], &argv[1]);
        perror("with-parent-death: execvp");
        return 127;
      }
    '';
  } ''
    install -d $out/bin
    cc -O2 -s -o $out/bin/with-parent-death $src
  '';
in
rec {
  inherit withParentDeath;
  mediaControl = mkWorkspacePackage "media-control" {
    nativeBuildInputs = [ pkgs.makeWrapper ];
    postInstall = ''
      wrapProgram "$out/bin/media-control" \
        --set MEDIA_CONTROL_PLAYERCTL "${pkgs.lib.getExe pkgs.playerctl}" \
        --set MEDIA_CONTROL_ROFI "${pkgs.lib.getExe pkgs.rofi}" \
        --set MEDIA_CONTROL_FALLBACK_THEME "$out/share/rofi/themes/media-control.rasi"
    '';
  };

  ocrScreenshot = mkWorkspacePackage "ocr-screenshot" {
    nativeBuildInputs = [ pkgs.makeWrapper ];
    postInstall = ''
      wrapProgram "$out/bin/ocr-screenshot" \
        --set OCR_SCREENSHOT_GNOME_SCREENSHOT "${pkgs.lib.getExe' pkgs.gnome-screenshot "gnome-screenshot"}" \
        --set OCR_SCREENSHOT_GRIM "${pkgs.lib.getExe' pkgs.grim "grim"}" \
        --set OCR_SCREENSHOT_SLURP "${pkgs.lib.getExe' pkgs.slurp "slurp"}" \
        --set OCR_SCREENSHOT_TESSERACT "${pkgs.lib.getExe' pkgs.tesseract "tesseract"}" \
        --set OCR_SCREENSHOT_WL_COPY "${pkgs.lib.getExe' pkgs.wl-clipboard "wl-copy"}" \
        --set OCR_SCREENSHOT_NOTIFY_SEND "${pkgs.lib.getExe' pkgs.libnotify "notify-send"}"
    '';
  };

  previewPanel = mkWorkspacePackage "preview-panel" {
    nativeBuildInputs = [
      pkgs.pkg-config
      pkgs.wrapGAppsHook4
    ];
    buildInputs = [
      pkgs.gtk4
      pkgs.gtk4-layer-shell
    ];
  };

  rofiFilesearch = mkWorkspacePackage "rofi-filesearch" {
    nativeBuildInputs = [ pkgs.makeWrapper ];
    postInstall = ''
      wrapProgram "$out/bin/rofi-filesearch" \
        --set ROFI_FILESEARCH_ROFI "${pkgs.lib.getExe pkgs.rofi}" \
        --set ROFI_FILESEARCH_FD "${pkgs.lib.getExe pkgs.fd}" \
        --set ROFI_FILESEARCH_GIO "${pkgs.lib.getExe' pkgs.glib "gio"}" \
        --set ROFI_FILESEARCH_XDG_OPEN "${pkgs.lib.getExe' pkgs.xdg-utils "xdg-open"}" \
        --set ROFI_FILESEARCH_DOLPHIN "${pkgs.lib.getExe pkgs.kdePackages.dolphin}" \
        --set ROFI_FILESEARCH_FILE "${pkgs.lib.getExe pkgs.file}" \
        --set ROFI_FILESEARCH_PDFTOPPM "${pkgs.lib.getExe' pkgs.poppler-utils "pdftoppm"}" \
        --set ROFI_FILESEARCH_FFMPEGTHUMBNAILER "${pkgs.lib.getExe pkgs.ffmpegthumbnailer}" \
        --set ROFI_FILESEARCH_PREVIEW_PANEL "${previewPanel}/bin/preview-panel"
    '';
  };

  rofiClipboard = mkWorkspacePackage "rofi-clipboard" {
    nativeBuildInputs = [ pkgs.makeWrapper ];
    postInstall = ''
      wrapProgram "$out/bin/rofi-clipboard" \
        --set ROFI_CLIPBOARD_ROFI "${pkgs.lib.getExe pkgs.rofi}" \
        --set ROFI_CLIPBOARD_PREVIEW_PANEL "${previewPanel}/bin/preview-panel" \
        --set ROFI_CLIPBOARD_WL_COPY "${pkgs.lib.getExe' pkgs.wl-clipboard "wl-copy"}" \
        --set ROFI_CLIPBOARD_WL_PASTE "${pkgs.lib.getExe' pkgs.wl-clipboard "wl-paste"}"
    '';
  };

  rofiNetworkManager = mkWorkspacePackage "rofi-network-manager" {
    nativeBuildInputs = [ pkgs.makeWrapper ];
    postInstall = ''
      wrapProgram "$out/bin/rofi-network-manager" \
        --set ROFI_NETWORK_ROFI "${pkgs.lib.getExe pkgs.rofi}" \
        --set ROFI_NETWORK_PREVIEW_PANEL "${previewPanel}/bin/preview-panel" \
        --set ROFI_NETWORK_NMCLI "${pkgs.lib.getExe' pkgs.networkmanager "nmcli"}" \
        --set ROFI_NETWORK_QRENCODE "${pkgs.lib.getExe' pkgs.qrencode "qrencode"}"
    '';
  };

  rofiAudio = mkWorkspacePackage "rofi-audio" {
    nativeBuildInputs = [
      pkgs.makeWrapper
      pkgs.pkg-config
    ];
    buildInputs = [
      pkgs.dbus
      pkgs.libpulseaudio
    ];
    postInstall = ''
      wrapProgram "$out/bin/rofi-audio" \
        --set ROFI_AUDIO_ROFI "${pkgs.lib.getExe pkgs.rofi}"
    '';
  };

  waybarTimer = mkWorkspacePackage "waybar-timer" {
    nativeBuildInputs = [ pkgs.makeWrapper ];
    postInstall = ''
      wrapProgram "$out/bin/waybar-timer" \
        --set WAYBAR_TIMER_FFPLAY "${pkgs.ffmpeg}/bin/ffplay"
    '';
  };

  webcamCrop = mkWorkspacePackage "webcam-crop" {
    nativeBuildInputs = [ pkgs.makeWrapper ];
    postInstall = ''
      wrapProgram "$out/bin/webcam-crop" \
        --set WEBCAM_CROP_FFMPEG "${pkgs.ffmpeg}/bin/ffmpeg" \
        --set WEBCAM_CROP_FUSER "${pkgs.psmisc}/bin/fuser" \
        --set WEBCAM_CROP_INOTIFYWAIT "${pkgs.inotify-tools}/bin/inotifywait" \
        --set WEBCAM_CROP_V4L2_CTL "${pkgs.v4l-utils}/bin/v4l2-ctl" \
        --set WEBCAM_CROP_V4L2LOOPBACK_CTL "${kernelPackages.v4l2loopback.bin}/bin/v4l2loopback-ctl"
    '';
  };

  niriWindowButtons = pkgs.rustPlatform.buildRustPackage rec {
    pname = "niri_window_buttons";
    version = "0.4.3";

    src = pkgs.fetchFromGitHub {
      owner = "adelmonte";
      repo = "niri_window_buttons";
      tag = "v${version}";
      hash = "sha256-CUeeDe5DY7IRf6pCl9g7q5rHNs4ca4mAg0eKgZ0ErlY=";
    };
    cargoHash = "sha256-STrFRNLgytpLilx0o/StCAnaO1dyWDUQDoTzb7PA2hc=";

    nativeBuildInputs = [ pkgs.pkg-config ];
    buildInputs = with pkgs; [
      glib
      gtk3
      cairo
      pango
      gdk-pixbuf
      atk
      libpulseaudio
    ];
    doCheck = false;

    meta = {
      description = "Waybar CFFI module for traditional window buttons in the niri compositor";
      homepage = "https://github.com/adelmonte/niri_window_buttons";
      license = pkgs.lib.licenses.gpl3Plus;
      platforms = pkgs.lib.platforms.linux;
    };
  };

  ycal =
    let
      inherit (pkgs) lib;
      src = pkgs.fetchzip {
        url = "https://github.com/yagybaba/waybar-ycal/archive/refs/tags/v1.1.0.tar.gz";
        sha256 = "0483nv1dspa7a90s8hxkb3kmva9r6c8qb61hilaks483n92lwf7a";
      };
      python = pkgs.python3;
      pythonWithDeps = python.withPackages (ps: [
        ps.google-api-python-client
        ps.google-auth
        ps.google-auth-oauthlib
        ps.google-auth-httplib2
        ps.pygobject3
      ]);
      typelibPath = lib.makeSearchPath "lib/girepository-1.0" [
        pkgs.gtk4
        pkgs.gtk4-layer-shell
        (lib.getLib pkgs.pango)
        pkgs.graphene
        pkgs.gobject-introspection
        pkgs.harfbuzz
        pkgs.gdk-pixbuf
      ];
      libraryPath = lib.makeLibraryPath [
        pkgs.gtk4
        pkgs.gtk4-layer-shell
        pkgs.pango
        pkgs.harfbuzz
      ];
      popupWrapper = pkgs.writeShellScriptBin "waybar-ycal-popup" ''
        #!/usr/bin/env sh
        set -euo pipefail

        OUT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
        POPUP_PY="$OUT_DIR/share/waybar-ycal/popup.py"

        export GI_TYPELIB_PATH="${typelibPath}"
        export LD_LIBRARY_PATH="${libraryPath}"
        exec "${pythonWithDeps}/bin/python" "$POPUP_PY" "$@"
      '';
      package = pkgs.stdenvNoCC.mkDerivation {
        pname = "waybar-ycal";
        version = "1.1.0";
        dontUnpack = true;
        nativeBuildInputs = [ python ];

        installPhase = ''
          mkdir -p $out/share/waybar-ycal $out/bin
          cp ${src}/bar.py ${src}/popup.py ${src}/toggle.sh $out/share/waybar-ycal/
          chmod +x $out/share/waybar-ycal/{bar.py,popup.py,toggle.sh}
          chmod u+w $out/share/waybar-ycal/popup.py

          python - <<PY
          import re
          from pathlib import Path

          p = Path('$out/share/waybar-ycal/popup.py')
          s = p.read_text()
          s = re.sub(
            r"def load_theme\(\):\n\s*defaults = \{[\s\S]*?\n\s*except Exception:\n\s*return defaults\n",
            "def load_theme():\n    return {\n        'foreground': '#cbe3e7',\n        'background': '#0E0616',\n        'accent': '#ff7edb',\n    }\n",
            s,
          )
          s = s.replace(
            "Gtk4LayerShell.set_anchor(self, Gtk4LayerShell.Edge.LEFT, False)",
            "Gtk4LayerShell.set_anchor(self, Gtk4LayerShell.Edge.LEFT, True)",
          )
          s = s.replace(
            "Gtk4LayerShell.set_margin(self, Gtk4LayerShell.Edge.TOP, 4)\n",
            "Gtk4LayerShell.set_margin(self, Gtk4LayerShell.Edge.TOP, 4)\n        Gtk4LayerShell.set_margin(self, Gtk4LayerShell.Edge.LEFT, 4)\n",
          )
          p.write_text(s)
          PY

          cp ${popupWrapper}/bin/waybar-ycal-popup $out/bin/waybar-ycal-popup
          chmod +x $out/bin/waybar-ycal-popup
        '';
      };
      barExec = "${pythonWithDeps}/bin/python ${package}/share/waybar-ycal/bar.py";
      toggle = pkgs.writeShellScript "waybar-ycal-toggle" ''
        set -euo pipefail
        PID_FILE="$HOME/.cache/waybar-ycal/popup.pid"

        if [ -f "$PID_FILE" ]; then
          PID="$(cat "$PID_FILE" 2>/dev/null || true)"
          if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
            kill -SIGUSR1 "$PID"
            exit 0
          fi
        fi

        systemctl --user start waybar-ycal.service >/dev/null 2>&1 || true
        for _ in 1 2 3 4 5; do
          if [ -f "$PID_FILE" ]; then
            PID="$(cat "$PID_FILE" 2>/dev/null || true)"
            if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
              kill -SIGUSR1 "$PID"
              exit 0
            fi
          fi
          sleep 0.2
        done
        ${package}/bin/waybar-ycal-popup &
      '';
    in
    {
      inherit package barExec toggle;
    };
}
