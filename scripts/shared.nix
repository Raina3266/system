# Shared Cargo workspace builds and AGS/Astal panel packaging.
{
  pkgs,
  craneLib,
  agsPackage,
  astalPackage,
  panelLauncher,
  workspaceRoot ? ./.,
  sourceRoot ? workspaceRoot,
}:
let
  inherit (pkgs) lib;

  # Cargo must be able to load every workspace member's manifest.
  workspace = (lib.importTOML (workspaceRoot + "/Cargo.toml")).workspace;
  workspaceMembers = workspace.members;

  # Keep the dependency cache and member builds in the same environment so
  # Cargo can reuse compiled dependencies, including their -sys build scripts.
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
        src = craneLib.cleanCargoSource workspaceRoot;
      }
      // extra
    );

  # Stub application sources before compiling dependencies, including dev-deps.
  cargoArtifacts = mkCargoArtifacts "desktop-scripts" { };

  # Include all manifests but only the requested member's Cargo sources.
  # UI assets and unrelated Rust edits must not invalidate this package.
  memberSrc =
    memberPath:
    lib.fileset.toSource {
      root = workspaceRoot;
      fileset = lib.fileset.unions [
        (craneLib.fileset.cargoTomlAndLock workspaceRoot)
        (craneLib.fileset.commonCargoSources (workspaceRoot + "/${memberPath}"))
      ];
    };

  # Cargo loads sibling manifests even with --package. Their empty targets
  # satisfy workspace discovery without compiling unrelated members.
  stubSiblings =
    memberPath:
    lib.concatMapStrings (m: ''
      mkdir -p ${m}/src
      : > ${m}/src/main.rs
    '') (lib.remove memberPath workspaceMembers);

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
        # Scope both compilation and tests to this member.
        cargoExtraArgs = "--locked --package ${cargoPackage}";
        inherit (commonArgs) strictDeps nativeBuildInputs buildInputs;
        postPatch = stubSiblings workspaceMember + (packageExtra.postPatch or "");
        meta = (packageExtra.meta or { }) // {
          inherit mainProgram;
        };
      }
    );

  mkPanel =
    {
      name,
      prefix,
      sourceDir,
      cssFile,
      palette ? null,
      entryPoint ? "frontend.js",
      extraPackages ? [ ],
      imageLoaders ? false,
      withPalette ? true,
      environment ? { },
      commands ? [ "run" "status" "toggle" "close" ],
      strictHelp ? true,
      checkResponse ? false,
      passthru ? { },
    }:
    assert (!withPalette) || palette != null;
    let
      mkAgsRuntime =
        { pkgs, ags }:
        ags.overrideAttrs (old: {
          postFixup = (old.postFixup or "") + ''
            makeWrapper ${pkgs.lib.getExe pkgs.gjs} "$out/bin/panel-gjs" \
              "''${gappsWrapperArgs[@]}" \
              --set LD_PRELOAD "${pkgs.gtk4-layer-shell}/lib/libgtk4-layer-shell.so"
          '';
        });

      # Only runtime code/metadata enter the snapshot. CSS stays live, and tests,
      # docs, and unrelated panels must not invalidate this launcher.
      source = lib.fileset.toSource {
        root = sourceRoot;
        fileset = lib.fileset.unions [
          (sourceRoot + "/live-style.js")
          (sourceRoot + "/ipc.js")
          (lib.fileset.fileFilter
            (file:
              (file.hasExt "js" || file.hasExt "ts" || file.hasExt "json")
              && !(lib.elem file.name [ "test.js" "test.ts" "tests.ts" ]))
            (sourceRoot + "/${sourceDir}"))
        ];
      };
      entry = "${source}/${sourceDir}/${entryPoint}";

      pixbufLoaders = pkgs.runCommand "panel-pixbuf-loaders.cache" { preferLocalBuild = true; } ''
        for package in ${lib.escapeShellArgs [ pkgs.gdk-pixbuf pkgs.librsvg pkgs.webp-pixbuf-loader pkgs.libavif ]}; do
          module_dir="$package/${pkgs.gdk-pixbuf.moduleDir}"
          test -d "$module_dir"
          GDK_PIXBUF_MODULEDIR="$module_dir" \
            ${lib.getDev pkgs.gdk-pixbuf}/bin/gdk-pixbuf-query-loaders
        done > "$out"
      '';
      ags = mkAgsRuntime {
        inherit pkgs;
        ags = (agsPackage.override {
          # AGS uses the GTK3 hook; declare the GTK4 typelibs explicitly.
          extraPackages = [ pkgs.gtk4 pkgs.graphene pkgs.gtk4-layer-shell ] ++ extraPackages;
        }).overrideAttrs (old: lib.optionalAttrs imageLoaders {
          postInstall = (old.postInstall or "") + ''
            export GDK_PIXBUF_MODULE_FILE=${pixbufLoaders}
          '';
        });
      };

      # Bundle immutable application code without a display or session bus.
      bundle = pkgs.runCommand "${name}-bundle.js" { } ''
        ${lib.getExe ags} bundle --gtk 4 -- ${lib.escapeShellArg entry} "$out"
        substituteInPlace "$out" \
          --replace-fail ${lib.escapeShellArg (lib.getExe pkgs.gjs)} ${lib.escapeShellArg "${ags}/bin/panel-gjs"} \
          --replace-fail '-ags.js"' '-${name}-ags.js"'
      '';
      paletteFile = pkgs.writeText "${name}-palette.json" (builtins.toJSON palette);
      launcherConfig = pkgs.writeText "${name}-launcher.json" (builtins.toJSON {
        inherit name commands;
        strict_help = strictHelp;
        check_response = checkResponse;
        astal = lib.getExe' astalPackage "astal";
        inherit bundle;
        environment = {
          PANEL_CSS_FILE = toString cssFile;
        } // lib.optionalAttrs withPalette {
          "${prefix}_PALETTE" = toString paletteFile;
        } // environment;
        # A service restart must not pick up unbuilt checkout JS/TS.
        forced_environment."${prefix}_CONFIG" = entry;
      });
    in
    pkgs.runCommand name {
      nativeBuildInputs = [ pkgs.makeBinaryWrapper ];
      meta.mainProgram = name;
      passthru = passthru // { inherit ags source entry bundle launcherConfig; }
        // lib.optionalAttrs withPalette { palette = paletteFile; };
    } ''
      install -d "$out/bin" "$out/share/${name}"
      ln -s ${launcherConfig} "$out/share/${name}/launcher.json"
      makeBinaryWrapper ${lib.getExe panelLauncher} "$out/bin/${name}" \
        --add-flags ${lib.escapeShellArg "--config ${launcherConfig}"}
    '';
in
{
  inherit mkCargoArtifacts mkWorkspacePackage mkPanel;
}
