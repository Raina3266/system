# Resident Quickshell config for the media panel and system dashboard.
# QML sources link live out of the repository; shared popup components and
# colors come from niri/quickshell/style.nix.
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
    "MediaController.qml"
    "MediaCredit.qml"
    "Dashboard.qml"
    "DashboardPanel.qml"
    "DashboardSection.qml"
    "PowerSection.qml"
    "SystemSection.qml"
    "SystemMetrics.qml"
    "NotificationSection.qml"
    "NotificationEntry.qml"
    "Notifications.qml"
    "NotificationPopups.qml"
    "NotificationStore.js"
    "NetworkController.qml"
    "NetworkFormat.js"
    "NetworkPanel.qml"
    "MediaStatus.js"
    "ActionTile.qml"
    "MetricBar.qml"
    "IdleInhibitorWindow.qml"
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
    repoPackages.mediaControl
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
      Description = "Quickshell panels and notification server";
      Conflicts = [ "swaync.service" ];
      X-Restart-Triggers = [ config.xdg.configFile."quickshell/media/shared/Colors.qml".source ];
      ConditionEnvironment = lib.mkForce [ "XDG_CURRENT_DESKTOP=niri" ];
      PartOf = [ "graphical-session.target" ];
      After = [ "graphical-session.target" ];
    };
    Service = {
      Type = "dbus";
      BusName = "org.freedesktop.Notifications";
      ExecStart = "${lib.getExe pkgs.quickshell} -c media";
      Restart = lib.mkForce "on-failure";
      RestartSec = 3;
      StateDirectory = "quickshell";
      StateDirectoryMode = "0700";
      UMask = "0077";
      # audio-control handles profile-aware media routing. System metrics
      # use native FileView readers plus coreutils df for filesystem capacity.
      Environment = [
        "AUDIO_CONTROL=${repoPackages.audioControl}/bin/audio-control"
        "MEDIA_CONTROL=${repoPackages.mediaControl}/bin/media-control"
        "COREUTILS_DF=${pkgs.coreutils}/bin/df"
        "NOTIFICATION_HISTORY=%S/quickshell/notifications.json"
        "NETWORK_HELPER=${repoPackages.networkManager}/bin/network-manager"
        "NETWORK_EDITOR=${pkgs.networkmanagerapplet}/bin/nm-connection-editor"
      ];
    };
    Install.WantedBy = [ "graphical-session.target" ];
  };
}
