# The media panel: a small resident Quickshell config whose card Waybar's
# centre button toggles. QML sources link live out of the repository (save to
# reload, like niri/config.kdl); only Colors.qml comes from the store,
# rendered from the same themes/palette.nix the Daemon themes are built with,
# so the panel cannot drift from the desktop's colours.
{
  config,
  lib,
  pkgs,
  repoRoot,
  repoPackages,
  ...
}:
let
  liveLink = path: config.lib.file.mkOutOfStoreSymlink "${repoRoot}/${path}";
  qmlFiles = [
    "shell.qml"
    "Routing.js"
    "MediaPanel.qml"
    "PlayerCard.qml"
    "Artwork.qml"
    "SeekBar.qml"
    "VolumeSlider.qml"
    "OutputList.qml"
    "SourcesList.qml"
  ];
in
{
  imports = [ ./style.nix ];

  home.packages = [
    pkgs.quickshell
    # WebP/JP2/ICO decoders for browser-published cover art (bilibili serves
    # .webp; Qt's base JPEG/PNG plugins already cover YouTube).
    pkgs.qt6.qtimageformats
  ];

  # The shared style module owns the palette, frame and controls.
  xdg.configFile = builtins.listToAttrs (
    map (name: {
      name = "quickshell/media/${name}";
      value.source = liveLink "themes/quickshell/${name}";
    }) qmlFiles
  );

  systemd.user.services.quickshell-media = {
    Unit = {
      Description = "Quickshell media panel";
      X-Restart-Triggers = [ config.xdg.configFile."quickshell/media/shared/Colors.qml".source ];
      ConditionEnvironment = lib.mkForce [ "XDG_CURRENT_DESKTOP=niri" ];
      PartOf = [ "graphical-session.target" ];
      After = [ "graphical-session.target" ];
    };
    Service = {
      ExecStart = "${lib.getExe pkgs.quickshell} -c media";
      Restart = lib.mkForce "on-failure";
      RestartSec = 3;
      # The panel spawns audio-control for route choices and the profile-aware
      # stream move; hand it the exact binary rather than relying on PATH.
      Environment = [ "AUDIO_CONTROL=${repoPackages.audioControl}/bin/audio-control" ];
    };
    Install.WantedBy = [ "graphical-session.target" ];
  };
}
