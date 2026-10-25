{ pkgs, clipboardCustom }:
let
  inherit (pkgs) lib;
  pixbufLoaders = pkgs.runCommand "clipboard-panel-pixbuf-loaders.cache" { preferLocalBuild = true; } ''
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
  ags = (import ./default.nix).mkAgsRuntime {
    inherit pkgs;
    ags = (pkgs.ags.override {
      extraPackages = [
        pkgs.gtk4
        pkgs.graphene
        pkgs.gtk4-layer-shell
      ];
    }).overrideAttrs (old: {
      postInstall = (old.postInstall or "") + ''
        export GDK_PIXBUF_MODULE_FILE=${pixbufLoaders}
      '';
    });
  };
  palette = pkgs.writeText "clipboard-panel-palette.json" (
    builtins.toJSON (import ../../themes/palette.nix)
  );
in
pkgs.writeShellApplication {
  name = "clipboard-panel";
  passthru = { inherit ags palette; };
  text = ''
    usage() {
      printf '%s\n' 'Usage: clipboard-panel run | status | toggle [monitor] | close'
    }
    command="''${1-}"
    case "$command" in
      -h|--help) usage; exit 0 ;;
      run|status|close)
        if [ "$#" -ne 1 ]; then usage >&2; exit 2; fi
        ;;
      toggle)
        if [ "$#" -gt 2 ]; then usage >&2; exit 2; fi
        ;;
      *) usage >&2; exit 2 ;;
    esac
    export CLIPBOARD_PANEL_CONFIG="''${CLIPBOARD_PANEL_CONFIG:-''${XDG_CONFIG_HOME:-$HOME/.config}/ags/clipboard/frontend.js}"
    export CLIPBOARD_PANEL_PALETTE="''${CLIPBOARD_PANEL_PALETTE:-${palette}}"
    export CLIPBOARD_CUSTOM_HELPER="''${CLIPBOARD_CUSTOM_HELPER:-${lib.getExe clipboardCustom}}"
    if [ "$command" = run ]; then
      runtime_dir="''${XDG_RUNTIME_DIR:?clipboard-panel: XDG_RUNTIME_DIR is required}/ags-panels"
      ${pkgs.coreutils}/bin/install -d -m 700 "$runtime_dir"
      bundle="$runtime_dir/clipboard-panel.js"
      ${lib.getExe ags} bundle --gtk 4 -- "$CLIPBOARD_PANEL_CONFIG" "$bundle"
      exec ${ags}/bin/panel-gjs -m "$bundle"
    fi
    request=$(${lib.getExe pkgs.jq} -cn --arg command "$command" --arg monitor "''${2-}" \
      '{command: $command} + (if $monitor == "" then {} else {monitor: $monitor} end)')
    response=$(${lib.getExe ags} request --instance clipboard-panel -- "$request")
    printf '%s\n' "$response"
    if printf '%s' "$response" | ${lib.getExe pkgs.jq} -e 'type == "object" and has("error")' >/dev/null 2>&1; then
      exit 1
    fi
  '';
}
