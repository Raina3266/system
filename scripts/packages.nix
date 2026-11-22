# Packages built from this repository and desktop integrations used by modules.
{
  pkgs,
  craneLib,
  agsPackage,
  astalPackages,
  niriWindowButtonsSrc,
  waybarYcalSrc,
  dolphinGdriveActionsSrc,
  kernelPackages ? pkgs.linuxPackages_latest,
  repoRoot ? "/home/raina/System",
}:
let
  inherit (pkgs) lib;
  panels = (import ../niri/panels.nix).packages;

  # Read from [workspace].members rather than repeated here: a member missing
  # from this list builds itself fine and breaks every other crate, because
  # cargo will not load a workspace whose members are absent from disk.
  workspace = (lib.importTOML ./Cargo.toml).workspace;
  workspaceMembers = workspace.members;

  # One build environment for the dependency artifact and every crate reusing
  # it. Cargo only reuses a compiled dependency when the environment matches,
  # and PKG_CONFIG_PATH comes from buildInputs — vary it per crate and that
  # crate's -sys deps rebuild alone. So: the union of what every member needs.
  commonArgs = {
    strictDeps = true;

    nativeBuildInputs = [
      pkgs.pkg-config
      pkgs.makeWrapper
    ];

    buildInputs = [ pkgs.dbus ];
  };

  mkCargoArtifacts =
    pname: extra:
    craneLib.buildDepsOnly (
      commonArgs
      // {
        inherit pname;
        version = workspace.package.version;
        src = craneLib.cleanCargoSource ./.;
      }
      // extra
    );

  # Every member's dependencies, compiled once. Crane stubs the crates' own
  # sources first, so editing any .rs leaves this alone; only a dependency or
  # Cargo.lock change rebuilds it. doCheck is on so dev-deps land here too.
  cargoArtifacts = mkCargoArtifacts "desktop-scripts" { };

  # Each member's source includes every workspace manifest but only that
  # member's Cargo sources, so unrelated source edits do not rebuild it.
  memberSrc =
    memberPath:
    lib.fileset.toSource {
      root = ./.;
      fileset = lib.fileset.unions [
        (craneLib.fileset.cargoTomlAndLock ./.)
        # Exclude UI/runtime assets (CSS, docs, screenshots) from the compiled
        # package input. Panel code is packaged separately; CSS stays live.
        (craneLib.fileset.commonCargoSources (./. + "/${memberPath}"))
      ];
    };

  # Every member manifest cargo loads needs a target to point at, so stand the
  # siblings back up as empty binaries. --package only walks the requested
  # member's dependency graph, so the stubs are parsed and never compiled.
  stubSiblings =
    memberPath:
    lib.concatMapStrings (m: ''
      mkdir -p ${m}/src
      : > ${m}/src/main.rs
    '') (lib.remove memberPath workspaceMembers);

  # Package and binary names match; nested helpers specify their workspace path.
  mkWorkspacePackage =
    pname: extra:
    let
      cargoPackage = extra.cargoPackage or pname;
      mainProgram = extra.mainProgram or pname;
      workspaceMember = extra.workspaceMember or pname;
      packageExtra = removeAttrs extra [
        "cargoPackage"
        "mainProgram"
        "workspaceMember"
      ];
    in
    craneLib.buildPackage (
      commonArgs
      // packageExtra
      // {
        inherit pname;
        cargoArtifacts = extra.cargoArtifacts or cargoArtifacts;
        version = workspace.package.version;
        src = memberSrc workspaceMember;
        # Feeds both the build and the test phase, so tests stay scoped to the
        # one member as well.
        cargoExtraArgs = "--locked --package ${cargoPackage}";
        # The shared environment has to win over anything a crate passes, or
        # that crate stops hitting the artifact cache.
        inherit (commonArgs) strictDeps nativeBuildInputs buildInputs;
        postPatch = stubSiblings workspaceMember + (packageExtra.postPatch or "");
        # Say which executable lib.getExe should resolve rather than making it
        # guess from the derivation name.
        meta = (packageExtra.meta or { }) // {
          inherit mainProgram;
        };
      }
    );
in
rec {
  withParentDeath = pkgs.writeShellScriptBin "with-parent-death" ''
    exec ${lib.getExe panelLauncher} runtime with-parent-death "$@"
  '';

  dolphinGdriveActions = pkgs.python3Packages.buildPythonApplication {
    pname = "dolphin-gdrive-actions";
    version = "0-unstable-${dolphinGdriveActionsSrc.shortRev}";
    pyproject = true;
    src = dolphinGdriveActionsSrc;

    build-system = [ pkgs.python3Packages.setuptools ];

    doCheck = true;
    nativeCheckInputs = [ pkgs.python3Packages.pytestCheckHook ];
    enabledTestPaths = [ "tests" ];
    preCheck = ''
      # Keep configuration and integration tests away from real user data.
      export HOME="$(mktemp -d)"
      export XDG_CONFIG_HOME="$HOME/.config"
      export XDG_DATA_HOME="$HOME/.local/share"
      export XDG_CACHE_HOME="$HOME/.cache"
      export RCLONE_CONFIG="$XDG_CONFIG_HOME/rclone/rclone.conf"
    '';
    pythonImportsCheck = [ "dolphin_gdrive_actions" ];

    # Only the installed CLI needs these tools; tests mock external services.
    makeWrapperArgs = [
      "--prefix"
      "PATH"
      ":"
      (lib.makeBinPath [
        pkgs.rclone
        pkgs.wl-clipboard
        pkgs.libnotify
      ])
    ];

    meta = {
      description = "Google Drive actions for rclone mounts";
      homepage = "https://github.com/sktk4/dolphin-gdrive-actions";
      license = lib.licenses.mit;
      mainProgram = "dga";
    };
  };

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

  clipboardCustom = mkWorkspacePackage "clipboard-custom" {
    workspaceMember = "clipboard/rust-custom";
    nativeCheckInputs = [ pkgs.glib.bin ];
    postInstall = ''
      wrapProgram "$out/bin/clipboard-custom" \
        --set CLIPBOARD_GIO "${pkgs.glib.bin}/bin/gio" \
        --set CLIPBOARD_WL_PASTE "${lib.getExe' pkgs.wl-clipboard "wl-paste"}"
    '';
    cargoArtifacts = mkCargoArtifacts "clipboard-custom-deps" {
      cargoExtraArgs = "--locked --package clipboard-custom";
    };
  };

  panelLauncher = mkWorkspacePackage "panel-launcher" {
    nativeCheckInputs = [ (lib.getBin pkgs.dbus) ];
    cargoArtifacts = mkCargoArtifacts "panel-launcher-deps" {
      cargoExtraArgs = "--locked --package panel-launcher";
    };
  };

  inherit (panels {
    inherit
      pkgs
      repoRoot
      panelLauncher
      audioControl
      mediaControl
      clipboardCustom
      agsPackage
      astalPackages
      ;
  }) clipboardPanel audioPanel mediaPanel networkPanel dashboardPanel appLauncher;

  # Resident audio/Bluetooth policy, native transaction orchestration, pairing,
  # compatibility discovery, and HID++ battery publication.
  audioControl = mkWorkspacePackage "audio-control" {
    workspaceMember = "audio-control/rust-custom";
    postInstall = ''
      wrapProgram "$out/bin/audio-control" \
        --set AUDIO_CONTROL_PW_DUMP "${lib.getExe' pkgs.pipewire "pw-dump"}"
    '';
  };

  # Resident media policy/metadata and private MPRIS recovery. AGS/Astal supplies
  # normal native snapshots and setters.
  mediaControl = mkWorkspacePackage "media-control" {
    workspaceMember = "media-control/rust-custom";
    postInstall = ''
      wrapProgram "$out/bin/media-control" \
        --set MEDIA_CONTROL_PLAYERCTL "${pkgs.playerctl}/bin/playerctl" \
        --set MEDIA_CONTROL_CURL "${lib.getExe pkgs.curl}"
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
        version = "unstable-${waybarYcalSrc.shortRev}";
        dontUnpack = true;
        nativeBuildInputs = [ python ];

        installPhase = ''
          mkdir -p $out/share/waybar-ycal $out/bin
          cp ${src}/bar.py ${src}/popup.py ${src}/toggle.sh $out/share/waybar-ycal/
          chmod +x $out/share/waybar-ycal/{bar.py,popup.py,toggle.sh}
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
          chmod +x $out/bin/waybar-ycal-popup
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
}
