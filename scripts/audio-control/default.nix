{
  pkgs,
  mkWorkspacePackage,
  mkPanel,
  repoRoot,
  palette,
  astalPackages,
}:
let
  # Resident audio/Bluetooth policy, transactions, pairing, and HID++ batteries.
  audioControl = mkWorkspacePackage "audio-control" {
    workspaceMember = "audio-control/rust-custom";
    postInstall = ''
      wrapProgram "$out/bin/audio-control" \
        --set AUDIO_CONTROL_PW_DUMP "${pkgs.lib.getExe' pkgs.pipewire "pw-dump"}"
    '';
    passthru = { inherit panel; };
  };

  panel = mkPanel {
    inherit palette;
    name = "audio-panel";
    prefix = "AUDIO_PANEL";
    sourceDir = "audio-control/astal";
    cssFile = "${repoRoot}/scripts/audio-control/astal/style.css";
    extraPackages = [ astalPackages.wireplumber astalPackages.bluetooth ];
    environment.AUDIO_CONTROL_HELPER = pkgs.lib.getExe audioControl;
    commands = [ "run" "status" "toggle" "close" "bluetoothPower" ];
    passthru.homeManagerModule =
      {
        config,
        lib,
        repoPackages,
        ...
      }:
      {
        home.packages = [
          repoPackages.audioPanel
          repoPackages.audioControl
        ];

        xdg.configFile."ags/audio-palette.json".source = repoPackages.audioPanel.palette;

        systemd.user.services.audio-control-ui = {
          Unit = {
            Description = "AGS/Astal GTK4 Bluetooth and audio panel";
            X-Restart-Triggers = [ config.xdg.configFile."ags/audio-palette.json".source ];
            ConditionEnvironment = [
              "WAYLAND_DISPLAY"
              "XDG_CURRENT_DESKTOP=niri"
            ];
            PartOf = [ "graphical-session.target" ];
            After = [ "graphical-session.target" ];
          };
          Service = {
            ExecStart = "${lib.getExe repoPackages.audioPanel} run";
            Environment = [
              "GDK_BACKEND=wayland"

              "AUDIO_PANEL_PALETTE=${config.xdg.configHome}/ags/audio-palette.json"
              "AUDIO_CONTROL_HELPER=${lib.getExe repoPackages.audioControl}"
            ];
            Restart = "on-failure";
            RestartSec = 3;
          };
          Install.WantedBy = [ "graphical-session.target" ];
        };
      };
  };
in
audioControl
