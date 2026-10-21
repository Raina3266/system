# Resident Quickshell dashboard, network panel, and notification server.
# Keep the legacy config/service name "media"; only media UI moved to AGS.
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
    "ActionTile.qml"
    "MetricBar.qml"
    "IdleInhibitorWindow.qml"
  ];
in
{
  imports = [ ./style.nix ];

  home.packages = [
    pkgs.quickshell
    # Retain extra decoders for notification images after media moves to GTK.
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
      # System metrics use native FileView readers plus coreutils df.
      # The launcher is invoked only when opening network, never for status.
      Environment = [
        "MEDIA_PANEL=${lib.getExe repoPackages.mediaPanel}"
        "COREUTILS_DF=${pkgs.coreutils}/bin/df"
        "NOTIFICATION_HISTORY=%S/quickshell/notifications.json"
        "NETWORK_HELPER=${repoPackages.networkManager}/bin/network-manager"
        "NETWORK_EDITOR=${pkgs.networkmanagerapplet}/bin/nm-connection-editor"
      ];
    };
    Install.WantedBy = [ "graphical-session.target" ];
  };
}
