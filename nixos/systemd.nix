# Central systemd configuration for both NixOS and Home Manager.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  webcam = config.services'.croppedWebcam;

  userSystemdModule =
    {
      config,
      lib,
      pkgs,
      ...
    }:
    let
      mkBisync =
        {
          name,
          localDir,
          remote,
          description,
          excludes ? [ ],
          timeout ? "2h",
          debounce ? 120,
        }:
        let
          unit = "rclone-bisync-${name}";
          stateDir = "${config.xdg.stateHome}/${unit}";

          rclone = "${pkgs.rclone}/bin/rclone";
          mkdir = "${pkgs.coreutils}/bin/mkdir -p";

          flags = [ "--drive-use-trash" ] ++ map (p: "--exclude '${p}'") excludes;

          excludeRegex = lib.concatMapStringsSep "|" (
            p: "^" + builtins.replaceStrings [ "." ] [ "\\." ] "${localDir}/${lib.removeSuffix "/**" p}" + "/"
          ) excludes;

          watchFlags = [
            "--monitor --recursive --quiet"
            "--event close_write,create,delete,move"
            "--format '%w%f'"
          ]
          ++ lib.optional (excludes != [ ]) "--excludei '${excludeRegex}'";

          syncScript = pkgs.writeShellScript "${unit}.sh" ''
            set -euo pipefail
            if [ -n "$(${pkgs.findutils}/bin/find '${stateDir}' -maxdepth 1 -name '*.lst' -print -quit 2>/dev/null)" ]; then
              resync=""
            else
              resync="--resync"
            fi

            exec ${rclone} bisync '${localDir}' '${remote}' \
              --workdir '${stateDir}' \
              ${lib.concatStringsSep " \\\n  " flags} \
              --conflict-resolve newer \
              --conflict-suffix conflict \
              --create-empty-src-dirs \
              --resilient --recover --max-lock 2m \
              --drive-skip-gdocs \
              --fast-list \
              --checkers 4 \
              --retries 3 --low-level-retries 10 \
              --timeout 30s --contimeout 30s \
              --tpslimit 10 --tpslimit-burst 20 \
              $resync
          '';

          watchScript = pkgs.writeShellScript "${unit}-watch.sh" ''
            set -euo pipefail

            ${pkgs.inotify-tools}/bin/inotifywait \
              ${lib.concatStringsSep " \\\n  " watchFlags} \
              '${localDir}' \
            | while read -r _changed; do
                while read -r -t ${toString debounce} _more; do :; done
                ${pkgs.systemd}/bin/systemctl --user start --no-block '${unit}.service'
              done
          '';

          baseService = {
            Type = "oneshot";
            Environment = "HOME=%h";
            Nice = 10;
            IOSchedulingClass = "best-effort";
            IOSchedulingPriority = 7;
            MemoryHigh = "512M";
          };
        in
        {
          services.${unit} = {
            Unit = {
              Description = description;
              After = [ "network-online.target" ];
              Wants = [ "network-online.target" ];
              X-SwitchMethod = "keep-old";
            };
            Service = baseService // {
              TimeoutStartSec = timeout;
              ExecStartPre = [
                "${mkdir} '${localDir}'"
                "${mkdir} '${stateDir}'"
              ];
              ExecStart = "${syncScript}";
            };
          };

          timers.${unit} = {
            Unit.Description = "Periodic ${description}";
            Timer = {
              OnBootSec = "2min";
              OnUnitActiveSec = "30min";
              Persistent = true;
            };
            Install.WantedBy = [ "timers.target" ];
          };

          services."${unit}-watch" = {
            Unit = {
              Description = "Watch ${localDir} and push changes to ${remote}";
              After = [ "${unit}.service" ];
            };
            Service = {
              Type = "simple";
              Environment = "HOME=%h";
              Nice = 10;
              IOSchedulingClass = "best-effort";
              IOSchedulingPriority = 7;
              ExecStartPre = "${mkdir} '${localDir}'";
              ExecStart = "${watchScript}";
              Restart = "on-failure";
              RestartSec = "30s";
            };
            Install.WantedBy = [ "default.target" ];
          };
        };

      syncs = map mkBisync (import ../home/cloud/bisync.nix { inherit config; });
      videoDir = "${config.home.homeDirectory}/Videos";
      fusermount = "/run/wrappers/bin/fusermount3";
      ycal = import ../niri/waybar/calender.nix { inherit pkgs; };

      qtEnvironment =
        let
          inherit (config.home) profileDirectory;
          qtPluginPath = qt: "${profileDirectory}/${qt.qtbase.qtPluginPrefix}";
          qtQmlPath = qt: "${profileDirectory}/${qt.qtbase.qtQmlPrefix}";
        in
        {
          QT_QPA_PLATFORMTHEME = "kde";
          QT_STYLE_OVERRIDE = "breeze";
          QT_PLUGIN_PATH = "${qtPluginPath pkgs.qt5}:${qtPluginPath pkgs.qt6}";
          QML2_IMPORT_PATH = "${qtQmlPath pkgs.qt5}:${qtQmlPath pkgs.qt6}";
        };
    in
    {
      config = lib.mkMerge [
        {
          systemd.user = {
            sessionVariables = qtEnvironment;

            services = lib.mkMerge (
              [
                {
                  bt-agent = {
                    Unit = {
                      Description = "Persistent Bluetooth pairing agent";
                      PartOf = [ "graphical-session.target" ];
                      After = [ "graphical-session.target" ];
                    };
                    Service = {
                      ExecStart = "${pkgs.bluez-tools}/bin/bt-agent --capability=DisplayYesNo";
                      Restart = "on-failure";
                    };
                    Install.WantedBy = [ "graphical-session.target" ];
                  };

                  kde-baloo = {
                    Unit = {
                      Description = "Baloo File Indexer";
                      PartOf = [ "graphical-session.target" ];
                      After = [ "graphical-session.target" ];
                    };
                    Service = {
                      ExecStart = "${pkgs.kdePackages.baloo}/libexec/kf6/baloo_file";
                      Restart = "on-failure";
                    };
                    Install.WantedBy = [ "graphical-session.target" ];
                  };

                  rclone-mount-video = {
                    Unit = {
                      Description = "Mount GoogleDrive:Video at ~/Videos (on-demand)";
                      After = [ "network-online.target" ];
                      Wants = [ "network-online.target" ];
                    };
                    Service = {
                      Type = "notify";
                      Environment = "HOME=%h";
                      Nice = 10;
                      IOSchedulingClass = "best-effort";
                      IOSchedulingPriority = 7;
                      TimeoutStartSec = "60s";
                      Restart = "on-failure";
                      RestartSec = "30s";
                      ExecStartPre = [
                        "${pkgs.coreutils}/bin/mkdir -p '${videoDir}'"
                        "-${fusermount} -uz '${videoDir}'"
                      ];
                      ExecStart = ''
                        ${pkgs.rclone}/bin/rclone mount GoogleDrive:Video '${videoDir}' \
                          --vfs-cache-mode full \
                          --vfs-cache-max-age 30m \
                          --vfs-cache-max-size 10G \
                          --vfs-cache-poll-interval 5m \
                          --vfs-read-chunk-size 8M \
                          --vfs-read-chunk-size-limit 256M \
                          --dir-cache-time 1h \
                          --poll-interval 10m \
                          --buffer-size 16M \
                          --retries 3 --low-level-retries 10 \
                          --timeout 30s --contimeout 30s \
                          --tpslimit 10 --tpslimit-burst 20
                      '';
                      ExecStop = "${fusermount} -u '${videoDir}'";
                    };
                    Install.WantedBy = [ "default.target" ];
                  };
                }
              ]
              ++ map (sync: sync.services) syncs
            );

            timers = lib.mkMerge (map (sync: sync.timers) syncs);
          };
        }

        (lib.mkIf config.programs'.waybar.enable {
          programs.waybar.systemd.enable = true;

          systemd.user.services.waybar = {
            Unit.ConditionEnvironment = lib.mkForce [ "XDG_CURRENT_DESKTOP=niri" ];
            Service = {
              Restart = lib.mkForce "on-failure";
              RestartSec = 3;
            };
          };

          systemd.user.services.waybar-ycal = {
            Unit = {
              Description = "waybar-ycal: Google Calendar and Tasks popup";
              ConditionEnvironment = lib.mkForce [ "XDG_CURRENT_DESKTOP=niri" ];
              PartOf = [ "graphical-session.target" ];
              After = [ "graphical-session.target" ];
            };
            Service = {
              ExecStart = "${ycal.waybarYcal}/bin/waybar-ycal-popup";
              Restart = lib.mkForce "on-failure";
              RestartSec = 3;
            };
            Install.WantedBy = [ "graphical-session.target" ];
          };
        })
      ];
    };
in
{
  home-manager.users.raina = {
    imports = [ userSystemdModule ];
  };

  # Immich and Jellyfin are installed, but start together only on demand.
  systemd.targets.media-stack = {
    description = "On-demand media services (Immich + Jellyfin)";
    unitConfig.StopWhenUnneeded = true;
  };

  systemd.services = {
    jellyfin = {
      wantedBy = lib.mkForce [ ];
      partOf = [ "media-stack.target" ];
    };
    immich-server = {
      wantedBy = lib.mkForce [ ];
      partOf = [ "media-stack.target" ];
    };
    immich-machine-learning = {
      wantedBy = lib.mkForce [ ];
      partOf = [ "media-stack.target" ];
    };
    redis-immich = {
      wantedBy = lib.mkForce [ ];
      partOf = [ "media-stack.target" ];
    };
    ensure-printers = {
      wantedBy = lib.mkForce [ ];
      restartIfChanged = false;
      stopIfChanged = false;
    };

    cropped-webcam = lib.mkIf webcam.enable {
      description = "Cropped virtual webcam supervisor (${webcam.source} -> /dev/video${toString webcam.videoNr})";
      wantedBy = [ "multi-user.target" ];
      after = [ "systemd-udev-settle.service" ];
      serviceConfig = {
        ExecStart = webcam.supervisor;
        Restart = "always";
        RestartSec = 2;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        NoNewPrivileges = true;
      };
    };
  };
}
