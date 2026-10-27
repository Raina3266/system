# Package definitions, launchers, and services for the AGS/Astal GTK4 panels.
rec {
  # ===========================================================================
  # Shared GTK / Astal runtime
  # ===========================================================================
  mkAgsRuntime =
    { pkgs, ags }:
    ags.overrideAttrs (old: {
      postFixup = (old.postFixup or "") + ''
        makeWrapper ${pkgs.lib.getExe pkgs.gjs} "$out/bin/panel-gjs" \
          "''${gappsWrapperArgs[@]}" \
          --set LD_PRELOAD "${pkgs.gtk4-layer-shell}/lib/libgtk4-layer-shell.so"
      '';
    });

  # ===========================================================================
  # Shared panel package and launcher builder
  # ===========================================================================
  mkPanel =
    {
      pkgs,
      repoRoot,
      panelLauncher,
      name,
      prefix,
      sourceDir,
      entryPoint ? "frontend.js",
      extraPackages ? [ ],
      imageLoaders ? false,
      withPalette ? true,
      environment ? { },
      commands ? [ "run" "status" "toggle" "close" ],
      strictHelp ? true,
      checkResponse ? false,
    }:
    let
      inherit (pkgs) lib;
      # --- Packaged application sources ----------------------------------------
      # Only runtime code/metadata enter the snapshot. CSS stays in the checkout;
      # docs and unrelated panels must not invalidate this launcher.
      source = lib.fileset.toSource {
        root = ../scripts;
        fileset = lib.fileset.unions [
          ../scripts/live-style.js
          (lib.fileset.fileFilter
            (file: file.hasExt "js" || file.hasExt "ts" || file.hasExt "json")
            (../scripts + "/${sourceDir}"))
        ];
      };
      entry = "${source}/${sourceDir}/${entryPoint}";
      # --- Image loaders and native runtime -----------------------------------
      pixbufLoaders = pkgs.runCommand "panel-pixbuf-loaders.cache" { preferLocalBuild = true; } ''
        for package in ${lib.escapeShellArgs [ pkgs.gdk-pixbuf pkgs.librsvg pkgs.webp-pixbuf-loader pkgs.libavif ]}; do
          module_dir="$package/${pkgs.gdk-pixbuf.moduleDir}"
          test -d "$module_dir"
          GDK_PIXBUF_MODULEDIR="$module_dir" \
            ${lib.getDev pkgs.gdk-pixbuf}/bin/gdk-pixbuf-query-loaders
        done > "$out"
      '';
      ags = mkAgsRuntime {
        inherit pkgs;
        ags = (pkgs.ags.override {
          # AGS uses the GTK3 hook; declare the GTK4 typelibs explicitly.
          extraPackages = [ pkgs.gtk4 pkgs.graphene pkgs.gtk4-layer-shell ] ++ extraPackages;
        }).overrideAttrs (old: lib.optionalAttrs imageLoaders {
          postInstall = (old.postInstall or "") + ''
            export GDK_PIXBUF_MODULE_FILE=${pixbufLoaders}
          '';
        });
      };
      # Bundle immutable application code once, without a display or session bus.
      # CSS is excluded from source and remains live through PANEL_CSS_FILE.
      bundle = pkgs.runCommand "${name}-bundle.js" { } ''
        ${lib.getExe ags} bundle --gtk 4 -- ${lib.escapeShellArg entry} "$out"
      '';
      # --- Generated palette --------------------------------------------------
      palette = pkgs.writeText "${name}-palette.json" (
        builtins.toJSON (import ../themes).palette
      );
      launcherConfig = pkgs.writeText "${name}-launcher.json" (builtins.toJSON {
        inherit name commands;
        strict_help = strictHelp;
        check_response = checkResponse;
        ags = lib.getExe ags;
        gjs = "${ags}/bin/panel-gjs";
        inherit bundle;
        environment = {
          PANEL_CSS_FILE = "${repoRoot}/scripts/${sourceDir}/style.css";
        } // lib.optionalAttrs withPalette {
          "${prefix}_PALETTE" = toString palette;
        } // environment;
        # Never pick up checkout JS/TS, even after a service restart.
        forced_environment."${prefix}_CONFIG" = entry;
      });
    in
    # Only the fixed manifest is injected here; all CLI behavior lives in Rust.
    pkgs.runCommand name {
      nativeBuildInputs = [ pkgs.makeBinaryWrapper ];
      meta.mainProgram = name;
      passthru = { inherit ags source entry bundle launcherConfig; }
        // lib.optionalAttrs withPalette { inherit palette; };
    } ''
      install -d "$out/bin" "$out/share/${name}"
      ln -s ${launcherConfig} "$out/share/${name}/launcher.json"
      makeBinaryWrapper ${lib.getExe panelLauncher} "$out/bin/${name}" \
        --add-flags ${lib.escapeShellArg "--config ${launcherConfig}"}
    '';

  # ===========================================================================
  # Per-panel package declarations
  # ===========================================================================
  packages =
    { pkgs, repoRoot, panelLauncher, audioControl, mediaControl, networkManager, clipboardCustom }:
    let
      panel = args: mkPanel ({ inherit pkgs repoRoot panelLauncher; } // args);
    in
    {
      # --- Audio / Bluetooth --------------------------------------------------
      audioPanel = panel {
        name = "audio-panel";
        prefix = "AUDIO_PANEL";
        sourceDir = "audio-control/astal";
        extraPackages = [ pkgs.astal.wireplumber pkgs.astal.bluetooth ];
        environment.AUDIO_CONTROL_HELPER = pkgs.lib.getExe audioControl;
        commands = [ "run" "status" "toggle" "close" "bluetoothPower" ];
      };
      # --- Media --------------------------------------------------------------
      mediaPanel = panel {
        name = "media-panel";
        prefix = "MEDIA_PANEL";
        sourceDir = "media-control/astal";
        extraPackages = [ pkgs.astal.mpris pkgs.astal.wireplumber ];
        imageLoaders = true;
        strictHelp = false;
        environment = {
          MEDIA_CONTROL = pkgs.lib.getExe mediaControl;
          NETWORK_PANEL = "network-panel";
        };
        commands = [ "run" "status" "toggle" "togglePauseAll" "cycleMedia" "pauseAll" "close" ];
      };
      # --- Network ------------------------------------------------------------
      networkPanel = panel {
        name = "network-panel";
        prefix = "NETWORK_PANEL";
        sourceDir = "network-manager/astal";
        # AstalNetwork's GIR references NM.Client; include libnm's typelib.
        extraPackages = [ pkgs.astal.network pkgs.networkmanager ];
        environment = {
          NETWORK_CUSTOM_HELPER = pkgs.lib.getExe networkManager;
          DASHBOARD_PANEL = "dashboard-panel";
          MEDIA_PANEL = "media-panel";
        };
        commands = [ "run" "status" "toggle" "close" "toggleWifi" ];
      };
      # --- Dashboard ----------------------------------------------------------
      dashboardPanel = panel {
        name = "dashboard-panel";
        prefix = "DASHBOARD_PANEL";
        sourceDir = "dashboard";
        entryPoint = "power.ts";
        extraPackages = [
          pkgs.astal.battery pkgs.astal.powerprofiles pkgs.astal.notifd
        ];
        imageLoaders = true;
        environment = {
          NETWORK_PANEL = "network-panel";
          COREUTILS_DF = "${pkgs.coreutils}/bin/df";
          SWAYIDLE = pkgs.lib.getExe pkgs.swayidle;
          SYSTEMCTL = "${pkgs.systemd}/bin/systemctl";
        };
        commands = [ "run" "status" "toggle" "close" "toggleDnd" "clearNotifications" ];
      };
      # --- Application launcher -----------------------------------------------
      appLauncher = panel {
        name = "app-launcher";
        prefix = "APP_LAUNCHER";
        sourceDir = "app-launcher";
        extraPackages = [ pkgs.astal.apps ];
        withPalette = false;
        environment.NIRI = pkgs.lib.getExe pkgs.niri;
        commands = [ "run" "toggle" "close" "status" ];
      };
      # --- Clipboard ----------------------------------------------------------
      clipboardPanel = panel {
        name = "clipboard-panel";
        prefix = "CLIPBOARD_PANEL";
        sourceDir = "clipboard/astal";
        imageLoaders = true;
        strictHelp = false;
        checkResponse = true;
        environment.CLIPBOARD_CUSTOM_HELPER = pkgs.lib.getExe clipboardCustom;
      };
    };

  # ===========================================================================
  # Home Manager integration
  # ===========================================================================
  module =
    {
      config,
      lib,
      pkgs,
      repoPackages,
      ...
    }:
    {
      # --- Installed packages -------------------------------------------------
      home.packages = [
        repoPackages.mediaPanel
        repoPackages.mediaControl
        repoPackages.audioPanel
        repoPackages.audioControl
        repoPackages.dashboardPanel
        repoPackages.networkPanel
        repoPackages.appLauncher
        repoPackages.clipboardPanel
        repoPackages.clipboardCustom
      ];

      # Inferred TypeScript projects discover ancestor @types packages without
      # a checkout tsconfig or node_modules. Keep generated dependencies in Nix.
      home.file = {
        "node_modules/@types/dashboard-gi".source =
          "${repoPackages.dashboardTypeEnvironment}/node_modules/@types/dashboard-gi";
        "node_modules/@types/node".source =
          "${repoPackages.dashboardTypeEnvironment}/node_modules/@types/node";
      };

      # --- Palette configuration files ----------------------------------------
      # CSS is read directly through PANEL_CSS_FILE. Installing child links under
      # legacy live-linked config directories would write into the checkout.
      xdg.configFile."ags/media-palette.json".source = repoPackages.mediaPanel.palette;
      xdg.configFile."ags/audio-palette.json".source = repoPackages.audioPanel.palette;
      xdg.configFile."ags/network-palette.json".source = repoPackages.networkPanel.palette;
      xdg.configFile."ags/dashboard-palette.json".source = repoPackages.dashboardPanel.palette;
      xdg.configFile."ags/clipboard-palette.json".source = repoPackages.clipboardPanel.palette;

      # --- Media service ------------------------------------------------------
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

            "MEDIA_PANEL_PALETTE=${config.xdg.configHome}/ags/media-palette.json"
            "MEDIA_CONTROL=${lib.getExe repoPackages.mediaControl}"
            "NETWORK_PANEL=${lib.getExe repoPackages.networkPanel}"
          ];
          Restart = lib.mkForce "on-failure";
          RestartSec = 3;
        };
        Install.WantedBy = [ "graphical-session.target" ];
      };

      # --- Network service ----------------------------------------------------
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

      # --- Audio / Bluetooth service ------------------------------------------
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

      # --- Dashboard / notification service -----------------------------------
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

      # --- Application launcher service ---------------------------------------
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

            "NIRI=${lib.getExe pkgs.niri}"
          ];
          Restart = "on-failure";
          RestartSec = 3;
        };
        Install.WantedBy = [ "graphical-session.target" ];
      };

      # One data-control watcher per session, independent of panels and Waybar.
      systemd.user.services.clipboard-collector = {
        Unit = {
          Description = "Capture Wayland clipboard history once per session";
          ConditionEnvironment = [
            "WAYLAND_DISPLAY"
            "XDG_CURRENT_DESKTOP=niri"
          ];
          PartOf = [ "graphical-session.target" ];
          After = [ "graphical-session.target" ];
        };
        Service = {
          ExecStart = "${lib.getExe' pkgs.wl-clipboard "wl-paste"} --no-newline --watch ${lib.getExe repoPackages.clipboardCustom} capture";
          UMask = "0077";
          Restart = "always";
          RestartSec = 2;
        };
        Install.WantedBy = [ "graphical-session.target" ];
      };

      # --- Clipboard service --------------------------------------------------
      systemd.user.services.ags-clipboard = {
        Unit = {
          Description = "AGS/Astal GTK4 clipboard history and preview panel";
          X-Restart-Triggers = [ config.xdg.configFile."ags/clipboard-palette.json".source ];
          ConditionEnvironment = [
            "WAYLAND_DISPLAY"
            "XDG_CURRENT_DESKTOP=niri"
          ];
          PartOf = [ "graphical-session.target" ];
          Wants = [ "clipboard-collector.service" ];
          After = [ "graphical-session.target" "clipboard-collector.service" ];
        };
        Service = {
          ExecStart = "${lib.getExe repoPackages.clipboardPanel} run";
          Environment = [
            "GDK_BACKEND=wayland"

            "CLIPBOARD_PANEL_PALETTE=${config.xdg.configHome}/ags/clipboard-palette.json"
          ];
          UMask = "0077";
          Restart = "on-failure";
          RestartSec = 3;
        };
        Install.WantedBy = [ "graphical-session.target" ];
      };
    };
}
