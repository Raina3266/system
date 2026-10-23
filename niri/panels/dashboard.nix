{ pkgs }:
let
  inherit (pkgs) lib;
  pixbufLoaders = pkgs.runCommand "dashboard-panel-pixbuf-loaders.cache" { preferLocalBuild = true; } ''
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
        pkgs.gtk4
        pkgs.graphene
        pkgs.gtk4-layer-shell
        pkgs.astal.battery
        pkgs.astal.powerprofiles
        pkgs.astal.network
        # AstalNetwork's GIR references NM.Client; include its typelib explicitly.
        pkgs.networkmanager
        pkgs.astal.bluetooth
        pkgs.astal.notifd
      ];
    }).overrideAttrs (old: {
      postInstall = (old.postInstall or "") + ''
        export GDK_PIXBUF_MODULE_FILE=${pixbufLoaders}
      '';
    });
  };
  palette = pkgs.writeText "dashboard-panel-palette.json" (
    builtins.toJSON (import ../../themes/palette.nix)
  );
in
pkgs.writeShellApplication {
  name = "dashboard-panel";
  passthru = { inherit ags palette; };
  text = ''
    usage() {
      printf '%s\n' \
        'Usage: dashboard-panel run | status | toggle [monitor] | close | toggleDnd | clearNotifications'
    }
    if [ "$#" -eq 0 ]; then
      usage >&2
      exit 2
    fi
    command="$1"
    case "$command" in
      -h|--help)
        if [ "$#" -ne 1 ]; then usage >&2; exit 2; fi
        usage
        exit 0
        ;;
      run|status|close|toggleDnd|clearNotifications)
        if [ "$#" -ne 1 ]; then usage >&2; exit 2; fi
        ;;
      toggle)
        if [ "$#" -gt 2 ]; then usage >&2; exit 2; fi
        ;;
      *) usage >&2; exit 2 ;;
    esac

    export DASHBOARD_PANEL_CONFIG="''${DASHBOARD_PANEL_CONFIG:-''${XDG_CONFIG_HOME:-$HOME/.config}/ags/dashboard/power.ts}"
    export DASHBOARD_PANEL_PALETTE="''${DASHBOARD_PANEL_PALETTE:-${palette}}"
    export NETWORK_PANEL="''${NETWORK_PANEL:-network-panel}"
    export COREUTILS_DF="''${COREUTILS_DF:-${pkgs.coreutils}/bin/df}"

    if [ "$command" = run ]; then
      runtime_dir="''${XDG_RUNTIME_DIR:?dashboard-panel: XDG_RUNTIME_DIR is required}/ags-panels"
      ${pkgs.coreutils}/bin/install -d -m 700 "$runtime_dir"
      bundle="$runtime_dir/dashboard-panel.js"
      ${lib.getExe ags} bundle --gtk 4 -- "$DASHBOARD_PANEL_CONFIG" "$bundle"
      exec ${ags}/bin/panel-gjs -m "$bundle"
    fi
    request=$(${lib.getExe pkgs.jq} -cn \
      --arg command "$command" --arg monitor "''${2-}" \
      '{command: $command} + (if $monitor == "" then {} else {monitor: $monitor} end)')
    exec ${lib.getExe ags} request --instance dashboard-panel -- "$request"
  '';
}
