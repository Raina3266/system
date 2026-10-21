{
  pkgs,
  audioControl,
}:
let
  inherit (pkgs) lib;
  ags = pkgs.ags.override {
    extraPackages = [
      # --gtk4 only preloads layer-shell. AGS uses wrapGAppsHook3, so GTK4's
      # typelibs must also be explicit inputs to its generated runtime wrapper.
      pkgs.gtk4
      pkgs.graphene
      pkgs.gtk4-layer-shell
      pkgs.astal.wireplumber
      pkgs.astal.bluetooth
    ];
  };
  palette = pkgs.writeText "audio-panel-palette.json" (
    builtins.toJSON (import ../../themes/palette.nix)
  );
in
pkgs.writeShellApplication {
  name = "audio-panel";
  passthru = { inherit ags palette; };
  text = ''
    usage() {
      printf '%s\n' \
        'Usage: audio-panel run | status | toggle [monitor] | close | bluetoothPower'
    }

    if [ "$#" -eq 0 ]; then
      usage >&2
      exit 2
    fi
    command="$1"
    case "$command" in
      -h|--help)
        if [ "$#" -ne 1 ]; then
          usage >&2
          exit 2
        fi
        usage
        exit 0
        ;;
      run|status|close|bluetoothPower)
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

    export AUDIO_PANEL_CONFIG="''${AUDIO_PANEL_CONFIG:-''${XDG_CONFIG_HOME:-$HOME/.config}/ags/audio/frontend.js}"
    export AUDIO_PANEL_PALETTE="''${AUDIO_PANEL_PALETTE:-${palette}}"
    export AUDIO_CONTROL_HELPER="''${AUDIO_CONTROL_HELPER:-${lib.getExe audioControl}}"

    if [ "$command" = run ]; then
      # AGS 2.3 has no run --instance flag; the application owns its instance name.
      exec ${lib.getExe ags} run --gtk4 -- "$AUDIO_PANEL_CONFIG"
    fi

    request=$(${lib.getExe pkgs.jq} -cn \
      --arg command "$command" \
      --arg monitor "''${2-}" \
      '{command: $command} + (if $monitor == "" then {} else {monitor: $monitor} end)')
    exec ${lib.getExe ags} request --instance audio-panel -- "$request"
  '';
}
