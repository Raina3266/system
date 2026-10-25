# Shared package runtime and Home Manager services for the AGS/Astal GTK4 panels.
{
  mkAgsRuntime =
    { pkgs, ags }:
    let
      inherit (pkgs) lib;
    in
    ags.overrideAttrs (old: {
      postFixup = (old.postFixup or "") + ''
        makeWrapper ${lib.getExe pkgs.gjs} "$out/bin/panel-gjs" \
          "''${gappsWrapperArgs[@]}" \
          --set LD_PRELOAD "${pkgs.gtk4-layer-shell}/lib/libgtk4-layer-shell.so"
      '';
    });

  module =
    {
      config,
      lib,
      pkgs,
      repoRoot,
      repoPackages,
      ...
    }:
    {
      home.packages = [
        repoPackages.mediaPanel
        repoPackages.mediaControl
        repoPackages.audioPanel
        repoPackages.audioControl
        repoPackages.dashboardPanel
        repoPackages.networkPanel
        repoPackages.appLauncher
        repoPackages.clipboardPanel
      ];

      # Keep generated assets outside this directory-level live link so Home
      # Manager never writes generated child links into the repository.
      xdg.configFile."ags/media".source =
        config.lib.file.mkOutOfStoreSymlink "${repoRoot}/scripts/media-control/astal";
      xdg.configFile."ags/media-palette.json".source = repoPackages.mediaPanel.palette;
      xdg.configFile."ags/audio".source =
        config.lib.file.mkOutOfStoreSymlink "${repoRoot}/scripts/audio-control/astal";
      xdg.configFile."ags/audio-palette.json".source = repoPackages.audioPanel.palette;
      xdg.configFile."ags/network".source =
        config.lib.file.mkOutOfStoreSymlink "${repoRoot}/scripts/network-manager/astal";
      xdg.configFile."ags/network-palette.json".source = repoPackages.networkPanel.palette;
      xdg.configFile."ags/dashboard".source =
        config.lib.file.mkOutOfStoreSymlink "${repoRoot}/scripts/dashboard";
      xdg.configFile."ags/dashboard-palette.json".source = repoPackages.dashboardPanel.palette;
      xdg.configFile."ags/app-launcher".source =
        config.lib.file.mkOutOfStoreSymlink "${repoRoot}/scripts/app-launcher";
      xdg.configFile."ags/clipboard".source =
        config.lib.file.mkOutOfStoreSymlink "${repoRoot}/scripts/clipboard/astal";
      xdg.configFile."ags/clipboard-palette.json".source = repoPackages.clipboardPanel.palette;

      systemd.user.services.ags-media = {
        Unit = {
          Description = "AGS/Astal GTK4 media panel";
          X-Restart-Triggers = [ config.xdg.configFile."ags/media-palette.json".source ];
          ConditionEnvironment = [
            "WAYLAND_DISPLAY"
            "XDG_CURRENT_DESKTOP=niri"
          ];
          PartOf = [ "graphical-session.target" ];
          After = [ "graphical-session.target" ];
        };
        Service = {
          ExecStart = "${lib.getExe repoPackages.mediaPanel} run";
          Environment = [
            "GDK_BACKEND=wayland"
            "MEDIA_PANEL_CONFIG=${config.xdg.configHome}/ags/media/frontend.js"
            "MEDIA_PANEL_PALETTE=${config.xdg.configHome}/ags/media-palette.json"
            "MEDIA_CONTROL=${lib.getExe repoPackages.mediaControl}"
            "NETWORK_PANEL=${lib.getExe repoPackages.networkPanel}"
          ];
          Restart = lib.mkForce "on-failure";
          RestartSec = 3;
        };
        Install.WantedBy = [ "graphical-session.target" ];
      };

      systemd.user.services.ags-network = {
        Unit = {
          Description = "AGS/Astal GTK4 network panel";
          X-Restart-Triggers = [ config.xdg.configFile."ags/network-palette.json".source ];
          ConditionEnvironment = [
            "WAYLAND_DISPLAY"
            "XDG_CURRENT_DESKTOP=niri"
          ];
          PartOf = [ "graphical-session.target" ];
          After = [ "graphical-session.target" ];
        };
        Service = {
          ExecStart = "${lib.getExe repoPackages.networkPanel} run";
          Environment = [
            "GDK_BACKEND=wayland"
            "NETWORK_PANEL_CONFIG=${config.xdg.configHome}/ags/network/frontend.js"
            "NETWORK_PANEL_PALETTE=${config.xdg.configHome}/ags/network-palette.json"
            "NETWORK_CUSTOM_HELPER=${lib.getExe repoPackages.networkManager}"

            "DASHBOARD_PANEL=${lib.getExe repoPackages.dashboardPanel}"
            "MEDIA_PANEL=${lib.getExe repoPackages.mediaPanel}"
          ];
          Restart = "on-failure";
          RestartSec = 3;
        };
        Install.WantedBy = [ "graphical-session.target" ];
      };

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
            "AUDIO_PANEL_CONFIG=${config.xdg.configHome}/ags/audio/frontend.js"
            "AUDIO_PANEL_PALETTE=${config.xdg.configHome}/ags/audio-palette.json"
            "AUDIO_CONTROL_HELPER=${lib.getExe repoPackages.audioControl}"
          ];
          Restart = "on-failure";
          RestartSec = 3;
        };
        Install.WantedBy = [ "graphical-session.target" ];
      };

      systemd.user.services.ags-dashboard = {
        Unit = {
          Description = "AGS/Astal GTK4 dashboard and notification server";
          Conflicts = [ "swaync.service" ];
          X-Restart-Triggers = [ config.xdg.configFile."ags/dashboard-palette.json".source ];
          ConditionEnvironment = [
            "WAYLAND_DISPLAY"
            "XDG_CURRENT_DESKTOP=niri"
          ];
          PartOf = [ "graphical-session.target" ];
          After = [ "graphical-session.target" ];
        };
        Service = {
          Type = "dbus";
          BusName = "org.freedesktop.Notifications";
          ExecStart = "${lib.getExe repoPackages.dashboardPanel} run";
          Environment = [
            "GDK_BACKEND=wayland"
            "DASHBOARD_PANEL_CONFIG=${config.xdg.configHome}/ags/dashboard/power.ts"
            "DASHBOARD_PANEL_PALETTE=${config.xdg.configHome}/ags/dashboard-palette.json"
            "NETWORK_PANEL=${lib.getExe repoPackages.networkPanel}"
            "NOTIFICATION_HISTORY=%S/dashboard/notifications.json"
            "LEGACY_NOTIFICATION_HISTORY=%S/quickshell/notifications.json"
          ];
          StateDirectory = "dashboard";
          StateDirectoryMode = "0700";
          UMask = "0077";
          Restart = "on-failure";
          RestartSec = 3;
        };
        Install.WantedBy = [ "graphical-session.target" ];
      };

      systemd.user.services.ags-app-launcher = {
        Unit = {
          Description = "AGS/Astal GTK4 app launcher";
          ConditionEnvironment = [
            "WAYLAND_DISPLAY"
            "XDG_CURRENT_DESKTOP=niri"
          ];
          PartOf = [ "graphical-session.target" ];
          After = [ "graphical-session.target" ];
        };
        Service = {
          ExecStart = "${lib.getExe repoPackages.appLauncher} run";
          Environment = [
            "GDK_BACKEND=wayland"
            "APP_LAUNCHER_CONFIG=${config.xdg.configHome}/ags/app-launcher/frontend.js"
            "NIRI=${lib.getExe pkgs.niri}"
          ];
          Restart = "on-failure";
          RestartSec = 3;
        };
        Install.WantedBy = [ "graphical-session.target" ];
      };

      systemd.user.services.ags-clipboard = {
        Unit = {
          Description = "AGS/Astal GTK4 clipboard history and preview panel";
          X-Restart-Triggers = [ config.xdg.configFile."ags/clipboard-palette.json".source ];
          ConditionEnvironment = [
            "WAYLAND_DISPLAY"
            "XDG_CURRENT_DESKTOP=niri"
          ];
          PartOf = [ "graphical-session.target" ];
          Wants = [ "rofi-clipboard-collector.service" ];
          After = [ "graphical-session.target" "rofi-clipboard-collector.service" ];
        };
        Service = {
          ExecStart = "${lib.getExe repoPackages.clipboardPanel} run";
          Environment = [
            "GDK_BACKEND=wayland"
            "CLIPBOARD_PANEL_CONFIG=${config.xdg.configHome}/ags/clipboard/frontend.js"
            "CLIPBOARD_PANEL_PALETTE=${config.xdg.configHome}/ags/clipboard-palette.json"
          ];
          UMask = "0077";
          Restart = "on-failure";
          RestartSec = 3;
        };
        Install.WantedBy = [ "graphical-session.target" ];
      };

      systemd.user.services.waybar.Unit = {
        Wants = [
          "audio-control-ui.service"
          "ags-dashboard.service"
          "ags-network.service"
          "ags-app-launcher.service"
          "ags-clipboard.service"
        ];
        After = [
          "audio-control-ui.service"
          "ags-dashboard.service"
          "ags-network.service"
          "ags-app-launcher.service"
          "ags-clipboard.service"
        ];
      };
    };
}
