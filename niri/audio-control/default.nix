{
  lib,
  config,
  pkgs,
  repoRoot,
  repoPackages,
  ...
}:
let
  qmlFiles = [
    "shell.qml"
    "AudioPage.qml"
    "BluetoothPage.qml"
  ];
in
{
  imports = [ ../quickshell/style.nix ];

  # Unmodified Quickshell; QML is live-linked just like the other desktop UI.
  home.packages = [ pkgs.quickshell ];
  # Remove the old directory-level source link before linking individual files.
  # Otherwise Home Manager follows it into the repo and writes its generated
  # file links over the QML sources, creating a symlink loop.
  home.activation.removeLegacyAudioControlDirectory =
    lib.hm.dag.entryBefore [ "linkGeneration" ] ''
      audio_control_config="$HOME/.config/quickshell/audio-control"
      if [ -L "$audio_control_config" ] \
        && [ "$(readlink -f "$audio_control_config")" = "${repoRoot}/niri/audio-control" ]; then
        $DRY_RUN_CMD rm "$audio_control_config"
      fi
    '';

  # Individual links leave room for the shared style aliases in this root.
  xdg.configFile = builtins.listToAttrs (
    map (name: {
      name = "quickshell/audio-control/${name}";
      value.source = config.lib.file.mkOutOfStoreSymlink "${repoRoot}/niri/audio-control/${name}";
    }) qmlFiles
  );

  systemd.user.services.audio-control-ui = {
    Unit = {
      Description = "Quickshell Bluetooth and audio panel";
      X-Restart-Triggers = [ config.xdg.configFile."quickshell/audio-control/shared/Colors.qml".source ];
      ConditionEnvironment = [
        "WAYLAND_DISPLAY"
        "XDG_CURRENT_DESKTOP=niri"
      ];
      PartOf = [ "graphical-session.target" ];
      After = [ "graphical-session.target" ];
    };
    Service = {
      ExecStart = "${pkgs.quickshell}/bin/quickshell --config audio-control";
      Environment = [
        "AUDIO_CONTROL_HELPER=${repoPackages.audioControl}/bin/audio-control"
        "QT_QUICK_CONTROLS_STYLE=Basic"
      ];
      Restart = "on-failure";
      RestartSec = 3;
    };
    Install.WantedBy = [ "graphical-session.target" ];
  };

  systemd.user.services.waybar.Unit = {
    Wants = [ "audio-control-ui.service" ];
    After = [ "audio-control-ui.service" ];
  };
}
