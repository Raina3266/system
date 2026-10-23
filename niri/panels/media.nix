{
  pkgs,
  mediaControl,
}:
let
  inherit (pkgs) lib;
  pixbufLoaders = pkgs.runCommand "media-panel-pixbuf-loaders.cache" { preferLocalBuild = true; } ''
    for package in ${
      lib.escapeShellArgs [
        pkgs.gdk-pixbuf
        pkgs.librsvg
        pkgs.webp-pixbuf-loader
        pkgs.libavif
      ]
    }; do
      module_dir="$package/${pkgs.gdk-pixbuf.moduleDir}"
      test -d "$module_dir"
      GDK_PIXBUF_MODULEDIR="$module_dir" \
        ${lib.getDev pkgs.gdk-pixbuf}/bin/gdk-pixbuf-query-loaders
    done > "$out"
  '';
  ags = import ./ags-runtime.nix {
    inherit pkgs;
    ags = (pkgs.ags.override {
      extraPackages = [
        # AGS uses wrapGAppsHook3, so GTK4's typelibs must also be explicit
        # inputs to its generated runtime wrapper.
        pkgs.gtk4
        pkgs.graphene
        pkgs.gtk4-layer-shell
        pkgs.astal.mpris
        pkgs.astal.wireplumber
      ];
    }).overrideAttrs
      (old: {
        # Setup hooks otherwise select librsvg's cache, not a union of loaders.
        # Override it after setup and before gappsWrapperArgsHook captures it.
        postInstall = (old.postInstall or "") + ''
          export GDK_PIXBUF_MODULE_FILE=${pixbufLoaders}
        '';
      });
  };
  palette = pkgs.writeText "media-panel-palette.json" (
    builtins.toJSON (import ../../themes/palette.nix)
  );
in
pkgs.writeShellApplication {
  name = "media-panel";
  passthru = { inherit ags palette; };
  text = ''
    usage() {
      printf '%s\n' \
        'Usage: media-panel run | status | toggle [monitor] | togglePauseAll | cycleMedia | pauseAll | close'
    }

    if [ "$#" -eq 0 ]; then
      usage >&2
      exit 2
    fi
    command="$1"
    case "$command" in
      -h|--help)
        usage
        exit 0
        ;;
      run|status|togglePauseAll|cycleMedia|pauseAll|close)
        if [ "$#" -ne 1 ]; then
          usage >&2
          exit 2
        fi
        ;;
      toggle)
        if [ "$#" -gt 2 ]; then
          usage >&2
          exit 2
        fi
        ;;
      *)
        usage >&2
        exit 2
        ;;
    esac

    export MEDIA_PANEL_CONFIG="''${MEDIA_PANEL_CONFIG:-''${XDG_CONFIG_HOME:-$HOME/.config}/ags/media/frontend.js}"
    export MEDIA_PANEL_PALETTE="''${MEDIA_PANEL_PALETTE:-${palette}}"
    export MEDIA_CONTROL="''${MEDIA_CONTROL:-${lib.getExe mediaControl}}"
    export NETWORK_PANEL="''${NETWORK_PANEL:-network-panel}"

    if [ "$command" = run ]; then
      runtime_dir="''${XDG_RUNTIME_DIR:?media-panel: XDG_RUNTIME_DIR is required}/ags-panels"
      ${pkgs.coreutils}/bin/install -d -m 700 "$runtime_dir"
      bundle="$runtime_dir/media-panel.js"
      ${lib.getExe ags} bundle --gtk 4 -- "$MEDIA_PANEL_CONFIG" "$bundle"
      exec ${ags}/bin/panel-gjs -m "$bundle"
    fi

    request=$(${lib.getExe pkgs.jq} -cn \
      --arg command "$command" \
      --arg monitor "''${2-}" \
      '{command: $command} + (if $monitor == "" then {} else {monitor: $monitor} end)')
    exec ${lib.getExe ags} request --instance media-panel -- "$request"
  '';
}
