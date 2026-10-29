# Packages built from this repository and desktop integrations used by modules.
{
  pkgs,
  craneLib,
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

  # Use AGS 2.3's generator revision and upstream lock, not npx's mutable
  # dependency resolution. Only generator workspaces are needed from this repo.
  tsForGir = pkgs.stdenvNoCC.mkDerivation (finalAttrs: {
    pname = "ts-for-gir";
    version = "4.0.0-beta.19";
    src = pkgs.fetchFromGitHub {
      owner = "gjsify";
      repo = "ts-for-gir";
      rev = "f1b7e7f5d158ad4a36815d6620aaec28486a0304";
      hash = "sha256-0yDAO1EC32TN+9rFiTHlw1jNV08m0Uiaq8DHD8Wh7v8=";
    };

    # Yarn omits checksums for optional platform dependencies. Keep the
    # supplemental metadata content-addressed without adding a checkout file.
    missingHashes = pkgs.runCommand "ts-for-gir-missing-hashes.json" {
      nativeBuildInputs = [ pkgs.yarn-berry.yarn-berry-fetcher ];
      SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
      outputHashMode = "flat";
      outputHashAlgo = "sha256";
      outputHash = "sha256-6kswIcTpVh9l4W+u7jxPtSSjuJVnNwgIrtl4CpbRAck=";
    } ''
      yarn-berry-fetcher missing-hashes ${finalAttrs.src}/yarn.lock > "$out"
    '';
    offlineCache = pkgs.yarn-berry.fetchYarnBerryDeps {
      inherit (finalAttrs) src missingHashes;
      hash = "sha256-GY09M2yc9ONAc+cri0QnALosh8nBTOJ3tmHRuoS1HuY=";
    };

    nativeBuildInputs = [ pkgs.nodejs pkgs.makeWrapper ];
    YARN_NODE_LINKER = "node-modules";
    YARN_ENABLE_SCRIPTS = "false";
    YARN_ENABLE_NETWORK = "false";
    YARN_ENABLE_TELEMETRY = "false";
    YARN_ENABLE_GLOBAL_CACHE = "false";
    # Prune the example/type workspaces while retaining all locked versions.
    YARN_ENABLE_IMMUTABLE_INSTALLS = "false";
    postPatch = ''
      node --input-type=commonjs -e '
        const fs = require("node:fs");
        const pkg = JSON.parse(fs.readFileSync("package.json", "utf8"));
        pkg.workspaces = ["packages/*"];
        fs.writeFileSync("package.json", JSON.stringify(pkg));
      '
    '';
    # The source vendors Yarn 4.5.1; newer Nixpkgs Yarn hooks reject its v8 lock.
    configurePhase = ''
      runHook preConfigure
      export HOME="$TMPDIR"
      cp -R ${finalAttrs.offlineCache}/cache .yarn/cache
      chmod -R u+w .yarn/cache
      node .yarn/releases/yarn-4.5.1.cjs install --mode=skip-build
      patchShebangs node_modules
      runHook postConfigure
    '';
    buildPhase = ''
      runHook preBuild
      for package in parser lib generator-base generator-typescript generator-html-doc cli; do
        node node_modules/typescript/bin/tsc -p "packages/$package"
      done
      runHook postBuild
    '';
    installPhase = ''
      runHook preInstall
      mkdir -p "$out/lib/ts-for-gir" "$out/bin"
      cp -R node_modules packages package.json "$out/lib/ts-for-gir/"
      makeWrapper ${lib.getExe pkgs.nodejs} "$out/bin/ts-for-gir" \
        --add-flags "$out/lib/ts-for-gir/packages/cli/lib/start.js"
      runHook postInstall
    '';
    meta.mainProgram = "ts-for-gir";
  });

  withParentDeath =
    pkgs.runCommandCC "with-parent-death"
      {
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
      }
      ''
        install -d $out/bin
        cc -O2 -s -o $out/bin/with-parent-death $src
      '';
in
rec {
  inherit withParentDeath;

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
    postInstall = ''
      wrapProgram "$out/bin/clipboard-custom" \
        --set CLIPBOARD_WL_PASTE "${lib.getExe' pkgs.wl-clipboard "wl-paste"}"
    '';
    cargoArtifacts = mkCargoArtifacts "clipboard-custom-deps" {
      cargoExtraArgs = "--locked --package clipboard-custom";
    };
  };

  panelLauncher = mkWorkspacePackage "panel-launcher" {
    cargoArtifacts = mkCargoArtifacts "panel-launcher-deps" {
      cargoExtraArgs = "--locked --package panel-launcher";
    };
  };

  inherit (panels {
    inherit pkgs repoRoot panelLauncher audioControl mediaControl clipboardCustom;
  }) clipboardPanel audioPanel mediaPanel networkPanel dashboardPanel appLauncher;

  dashboardTypeEnvironment =
    let
      # Match AGS's GIR search path, including propagated dev outputs. Avoid
      # GTK3 roots even though the runtime wrapper supports both GTK versions.
      depsOf = pkg: [ (pkg.dev or pkg) ] ++ map depsOf (pkg.propagatedBuildInputs or [ ]);
      girDirs = pkgs.symlinkJoin {
        name = "gir-dirs";
        paths = lib.flatten (map depsOf dashboardPanel.ags.buildInputs);
      };
      modules = [
        "GLib-2.0" "Gio-2.0" "GObject-2.0" "Gtk-4.0" "Gdk-4.0"
        "Astal-4.0" "AstalIO-0.1" "AstalBattery-0.1" "AstalPowerProfiles-0.1"
        "AstalNotifd-0.1"
        "Gtk4LayerShell-1.0" "Pango-1.0" "cairo-1.0"
      ];
      nodeTypes = pkgs.fetchurl {
        url = "https://registry.npmjs.org/@types/node/-/node-24.0.0.tgz";
        hash = "sha256-Jf0KcgqaKYySA9NasX0HFOP5Zc3lq7Cp6OBheofolZY=";
      };
      undiciTypes = pkgs.fetchurl {
        url = "https://registry.npmjs.org/undici-types/-/undici-types-7.8.0.tgz";
        hash = "sha256-lkv9/F7dwcBQcdlc7Tsd6VJtCgaHlFEhTorC1x8ubYo=";
      };
    in
    pkgs.runCommand "dashboard-type-environment" {
      nativeBuildInputs = [ tsForGir ];
      passthru = {
        generator = tsForGir;
        typescript = pkgs.typescript_5;
        inherit girDirs modules;
      };
    } ''
      mkdir -p "$out/node_modules/@types/node" "$out/node_modules/undici-types"
      # Copy, rather than symlink, so realpath-based dependency resolution finds
      # undici-types beside @types/node inside the same store environment.
      tar -xf ${nodeTypes} --strip-components=1 -C "$out/node_modules/@types/node"
      tar -xf ${undiciTypes} --strip-components=1 -C "$out/node_modules/undici-types"
      ts-for-gir generate ${lib.escapeShellArgs modules} \
        -g ${girDirs}/share/gir-1.0 \
        --outdir "$out/node_modules/@types/dashboard-gi"
      # This inferred project also loads Node types. Keep GJS's optional DOM
      # globals and bare console module available, but do not auto-load their
      # incompatible duplicates of Node/browser globals in the shared entry.
      substituteInPlace "$out/node_modules/@types/dashboard-gi/index.d.ts" \
        --replace-fail "import './dom.d.ts';" "" \
        --replace-fail "import './console.d.ts';" ""
      # GJS supplies its Cairo wrapper to GTK draw callbacks, not the opaque
      # GIR cairo.Context emitted by this generator revision.
      substituteInPlace "$out/node_modules/@types/dashboard-gi/gtk-4.0.d.ts" \
        --replace-fail "import type cairo from 'gi://cairo?version=1.0';" \
          "import type cairo from 'gi://cairo?version=1.0'; import type GjsCairo from 'cairo';" \
        --replace-fail "(drawing_area: DrawingArea, cr: cairo.Context, width: number, height: number): void;" \
          "(drawing_area: DrawingArea, cr: GjsCairo.Context, width: number, height: number): void;"
      test -s "$out/node_modules/@types/dashboard-gi/index.d.ts"
      test -s "$out/node_modules/@types/dashboard-gi/gtk-4.0.d.ts"
    '';

  # Headless audio compatibility, pairing, and HID++ battery publication only.
  audioControl = mkWorkspacePackage "audio-control" {
    workspaceMember = "audio-control/rust-custom";
    postInstall = ''
      wrapProgram "$out/bin/audio-control" \
        --set AUDIO_CONTROL_PW_DUMP "${lib.getExe' pkgs.pipewire "pw-dump"}"
    '';
  };

  # Custom media policy/metadata only; AGS/Astal supplies the native services.
  mediaControl = mkWorkspacePackage "media-control" {
    workspaceMember = "media-control/rust-custom";
    postInstall = ''
      wrapProgram "$out/bin/media-control" \
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
      inherit package bar toggle;
    };
}
