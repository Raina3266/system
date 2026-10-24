{
  pkgs,
  networkManager,
}:
let
  inherit (pkgs) lib;
  ags = (import ./default.nix).mkAgsRuntime {
    inherit pkgs;
    ags = pkgs.ags.override {
      extraPackages = [
        pkgs.gtk4
        pkgs.graphene
        pkgs.gtk4-layer-shell
        pkgs.astal.network
        # AstalNetwork's GIR exposes NM.Client; include libnm's typelib as well.
        pkgs.networkmanager
      ];
    };
  };
  palette = pkgs.writeText "network-panel-palette.json" (
    builtins.toJSON (import ../../themes/palette.nix)
  );
in
pkgs.writeShellApplication {
  name = "network-panel";
  passthru = { inherit ags palette; };
  text = ''
    usage() {
      printf '%s\n' 'Usage: network-panel run | status | toggle [monitor] | close | toggleWifi'
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
      run|status|close|toggleWifi)
        if [ "$#" -ne 1 ]; then usage >&2; exit 2; fi
        ;;
      toggle)
        if [ "$#" -gt 2 ]; then usage >&2; exit 2; fi
        ;;
      *) usage >&2; exit 2 ;;
    esac

    export NETWORK_PANEL_CONFIG="''${NETWORK_PANEL_CONFIG:-''${XDG_CONFIG_HOME:-$HOME/.config}/ags/network/frontend.js}"
    export NETWORK_PANEL_PALETTE="''${NETWORK_PANEL_PALETTE:-${palette}}"
    export NETWORK_CUSTOM_HELPER="''${NETWORK_CUSTOM_HELPER:-${lib.getExe networkManager}}"

    export DASHBOARD_PANEL="''${DASHBOARD_PANEL:-dashboard-panel}"
    export MEDIA_PANEL="''${MEDIA_PANEL:-media-panel}"

    if [ "$command" = run ]; then
      runtime_dir="''${XDG_RUNTIME_DIR:?network-panel: XDG_RUNTIME_DIR is required}/ags-panels"
      ${pkgs.coreutils}/bin/install -d -m 700 "$runtime_dir"
      bundle="$runtime_dir/network-panel.js"
      ${lib.getExe ags} bundle --gtk 4 -- "$NETWORK_PANEL_CONFIG" "$bundle"
      exec ${ags}/bin/panel-gjs -m "$bundle"
    fi
    request=$(${lib.getExe pkgs.jq} -cn \
      --arg command "$command" --arg monitor "''${2-}" \
      '{command: $command} + (if $monitor == "" then {} else {monitor: $monitor} end)')
    exec ${lib.getExe ags} request --instance network-panel -- "$request"
  '';
}
