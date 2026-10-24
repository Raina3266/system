{ pkgs }:
let
  inherit (pkgs) lib;
  ags = import ./ags-runtime.nix {
    inherit pkgs;
    ags = pkgs.ags.override {
      extraPackages = [
        pkgs.gtk4
        pkgs.graphene
        pkgs.gtk4-layer-shell
        pkgs.astal.apps
      ];
    };
  };
in
pkgs.writeShellApplication {
  name = "app-launcher";
  passthru = { inherit ags; };
  text = ''
    usage() {
      printf '%s\n' 'Usage: app-launcher run | toggle [monitor] | close | status'
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
      run|close|status)
        if [ "$#" -ne 1 ]; then usage >&2; exit 2; fi
        ;;
      toggle)
        if [ "$#" -gt 2 ]; then usage >&2; exit 2; fi
        ;;
      *) usage >&2; exit 2 ;;
    esac

    export APP_LAUNCHER_CONFIG="''${APP_LAUNCHER_CONFIG:-''${XDG_CONFIG_HOME:-$HOME/.config}/ags/app-launcher/frontend.js}"
    export NIRI="''${NIRI:-${lib.getExe pkgs.niri}}"

    if [ "$command" = run ]; then
      runtime_dir="''${XDG_RUNTIME_DIR:?app-launcher: XDG_RUNTIME_DIR is required}/ags-panels"
      ${pkgs.coreutils}/bin/install -d -m 700 "$runtime_dir"
      bundle="$runtime_dir/app-launcher.js"
      ${lib.getExe ags} bundle --gtk 4 -- "$APP_LAUNCHER_CONFIG" "$bundle"
      exec ${ags}/bin/panel-gjs -m "$bundle"
    fi
    request=$(${lib.getExe pkgs.jq} -cn \
      --arg command "$command" --arg monitor "''${2-}" \
      '{command: $command} + (if $monitor == "" then {} else {monitor: $monitor} end)')
    exec ${lib.getExe ags} request --instance app-launcher -- "$request"
  '';
}
