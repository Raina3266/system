# Utility packages and per-panel imports; common builders live in shared.nix.
{
  pkgs,
  craneLib,
  agsPackage,
  astalPackages,
  niriWindowButtonsSrc,
  waybarYcalSrc,
  kernelPackages ? pkgs.linuxPackages_latest,
  repoRoot ? "/home/raina/System",
}:
let
  inherit (pkgs) lib;
  inherit (import ./shared.nix {
    inherit pkgs craneLib agsPackage;
    astalPackage = astalPackages.io;
    panelLauncher = packages.panelLauncher;
  })
    mkWorkspacePackage
    mkCargoArtifacts
    mkPanel
    ;
  palette = (import ../themes).palette;

  packages = rec {
    ocrScreenshot = mkWorkspacePackage "ocr-screenshot" {
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
    waybarTimer = mkWorkspacePackage "waybar-timer" {
      postInstall = ''
        wrapProgram "$out/bin/waybar-timer" \
          --set WAYBAR_TIMER_FFPLAY "${pkgs.ffmpeg-full}/bin/ffplay"
      '';
    };
    webcamCrop = mkWorkspacePackage "webcam-crop" {
      postInstall = ''
        wrapProgram "$out/bin/webcam-crop" \
          --set WEBCAM_CROP_FFMPEG "${pkgs.ffmpeg-full}/bin/ffmpeg" \
          --set WEBCAM_CROP_FUSER "${pkgs.psmisc}/bin/fuser" \
          --set WEBCAM_CROP_INOTIFYWAIT "${pkgs.inotify-tools}/bin/inotifywait" \
          --set WEBCAM_CROP_V4L2_CTL "${pkgs.lib.getExe' pkgs.v4l-utils "v4l2-ctl"}" \
          --set WEBCAM_CROP_V4L2LOOPBACK_CTL "${kernelPackages.v4l2loopback.bin}/bin/v4l2loopback-ctl"
      '';
    };
    panelLauncher = import ./panel-launcher { inherit pkgs mkWorkspacePackage mkCargoArtifacts; };
    audioControl = import ./audio-control {
      inherit pkgs mkWorkspacePackage mkPanel repoRoot palette astalPackages;
    };
    mediaControl = import ./media-control {
      inherit pkgs mkWorkspacePackage mkPanel repoRoot palette astalPackages;
    };
    clipboardCustom = import ./clipboard {
      inherit pkgs mkWorkspacePackage mkCargoArtifacts mkPanel repoRoot palette;
    };
    withParentDeath = panelLauncher.withParentDeath;

    audioPanel = audioControl.panel;
    mediaPanel = mediaControl.panel;
    clipboardPanel = clipboardCustom.panel;
    networkPanel = import ./network-manager {
      inherit pkgs mkWorkspacePackage mkPanel repoRoot palette;
    };
    networkControl = networkPanel.backend;
    dashboardControl = import ./dashboard {
      inherit pkgs mkWorkspacePackage mkPanel repoRoot palette astalPackages;
    };
    dashboardPanel = dashboardControl.panel;
    appLauncher = import ./app-launcher {
      inherit pkgs mkPanel repoRoot astalPackages panelLauncher;
    };

    niriWindowButtons = pkgs.rustPlatform.buildRustPackage {
      pname = "niri_window_buttons";
      version = (lib.importTOML "${niriWindowButtonsSrc}/Cargo.toml").package.version;
      src = niriWindowButtonsSrc;
      cargoLock.lockFile = "${niriWindowButtonsSrc}/Cargo.lock";

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
        src = waybarYcalSrc;
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
          set -euo pipefail

          OUT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
          POPUP_PY="$OUT_DIR/share/waybar-ycal/popup.py"

          export GI_TYPELIB_PATH="${typelibPath}"
          export LD_LIBRARY_PATH="${libraryPath}"
          exec "${pythonWithDeps}/bin/python" "$POPUP_PY" "$@"
        '';
        package = pkgs.stdenvNoCC.mkDerivation {
          pname = "waybar-ycal";
          version = "unstable-${src.shortRev}";
          dontUnpack = true;
          nativeBuildInputs = [ python ];

          installPhase = ''
            mkdir -p $out/share/waybar-ycal $out/bin
            cp ${src}/bar.py ${src}/popup.py $out/share/waybar-ycal/
            chmod u+w $out/share/waybar-ycal/{bar.py,popup.py}

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
            # The popup is a self-styled layer surface that has to match Waybar,
            # not the desktop GTK theme, so its own sheet has to outrank
            # gtk-4.0/gtk.css. Upstream registers it at APPLICATION priority,
            # below the USER priority GTK gives that file, so Daemon's
            # "button:not(.flat)" frame rule won the cascade and drew a red
            # outline around every day cell, the month arrows and Add event.
            s = s.replace(
              "self._css_provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION",
              "self._css_provider, Gtk.STYLE_PROVIDER_PRIORITY_USER + 1",
            )
            p.write_text(s)

            p = Path('$out/share/waybar-ycal/bar.py')
            s = p.read_text()
            s = s.replace("%A %H:%M", "%A %-d %b %H:%M")
            p.write_text(s)
            PY

            cp ${popupWrapper}/bin/waybar-ycal-popup $out/bin/waybar-ycal-popup
          '';
        };
        bar = pkgs.writeShellScriptBin "waybar-ycal-bar" ''
          exec ${pythonWithDeps}/bin/python ${package}/share/waybar-ycal/bar.py "$@"
        '';
        toggle = pkgs.writeShellScriptBin "waybar-ycal-toggle" ''
          exec ${lib.getExe panelLauncher} runtime ycal-toggle "${pkgs.systemd}/bin/systemctl" \
            "${python}/bin/python" "${package}/share/waybar-ycal/popup.py" "${package}/bin/waybar-ycal-popup"
        '';
      in
      {
        inherit package bar toggle;
      };
  };
in
packages
