# Cloud sync and mounts: rclone bisync for GoogleDrive / Music
# plus an on-demand rclone mount of GoogleDrive:Video at ~/Videos.
{
  config,
  lib,
  pkgs,
  repoPackages,
  ...
}:
let
  syncSpecs = [
    {
      name = "Gdrive";
      localDir = "${config.home.homeDirectory}/GoogleDrive";
      remote = "GoogleDrive:";
      description = "bisync GoogleDrive";
      timeout = "6h";
      excludes = [
        "Shared/**"
        "Video/**"
        "Music/**"
        ".Trash-1000/**"
      ];
    }
    {
      name = "Music";
      localDir = "${config.home.homeDirectory}/Music";
      remote = "GoogleDrive:Music";
      description = "bisync GoogleDrive:Music";
    }
  ];

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

      flags = [ "--drive-use-trash" ] ++ map (pattern: "--exclude '${pattern}'") excludes;
      excludeRegex = lib.concatMapStringsSep "|" (
        pattern:
        "^"
        + builtins.replaceStrings [ "." ] [ "\\." ] "${localDir}/${lib.removeSuffix "/**" pattern}"
        + "/"
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
          ${lib.concatStringsSep " " flags} \
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
          ${lib.concatStringsSep " " watchFlags} \
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
      services = {
        ${unit} = {
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

        "${unit}-watch" = {
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

      timers.${unit} = {
        Unit.Description = "Periodic ${description}";
        Timer = {
          OnBootSec = "2min";
          OnUnitActiveSec = "30min";
          Persistent = true;
        };
        Install.WantedBy = [ "timers.target" ];
      };
    };

  syncs = map mkBisync syncSpecs;
  videoDir = "${config.home.homeDirectory}/Videos";
  fusermount = "/run/wrappers/bin/fusermount3";

  driveActionSpecs = syncSpecs ++ [
    {
      name = "Video";
      localDir = videoDir;
      remote = "GoogleDrive:Video";
    }
  ];
  driveActionsConfig = (pkgs.formats.toml { }).generate "dolphin-gdrive-actions.toml" {
    mounts = map (spec: {
      local_root = spec.localDir;
      remote = "Dga${spec.name}";
    }) driveActionSpecs;
  };
  # Upstream accepts remote names, not remote subpaths. Private aliases retain
  # the existing connection without editing the credential-bearing rclone config.
  dolphinGdriveActions = pkgs.symlinkJoin {
    name = "dolphin-gdrive-actions";
    meta.mainProgram = "dga";
    paths = [ repoPackages.dolphinGdriveActions ];
    nativeBuildInputs = [ pkgs.makeWrapper ];
    postBuild = ''
      wrapProgram "$out/bin/dga" \
        ${lib.concatMapStringsSep " \\\n        " (spec: "--set RCLONE_CONFIG_DGA${lib.toUpper spec.name}_TYPE alias --set RCLONE_CONFIG_DGA${lib.toUpper spec.name}_REMOTE ${lib.escapeShellArg spec.remote}") driveActionSpecs}
    '';
  };
in
{
  # fuse3 is not listed: the units below call /run/wrappers/bin/fusermount3,
  # the setuid wrapper NixOS provides, never an unwrapped copy from PATH.
  home.packages = [
    pkgs.rclone
    dolphinGdriveActions
  ];

  xdg.configFile."dolphin-gdrive-actions/config.toml".source = driveActionsConfig;

  xdg.dataFile."kio/servicemenus/copy-google-drive-link.desktop" = {
    executable = true;
    text = ''
      [Desktop Entry]
      Type=Service
      MimeType=application/octet-stream;inode/directory;
      Actions=copyGoogleDriveLink;
      X-KDE-Protocol=file
      X-KDE-Priority=TopLevel
      X-KDE-RequiredNumberOfUrls=1

      [Desktop Action copyGoogleDriveLink]
      Name=Copy Google Drive Link
      Icon=edit-copy
      Exec=${lib.getExe dolphinGdriveActions} copy-link --config ${driveActionsConfig} --clipboard %f
    '';
  };

  systemd.user = {
    services = lib.mkMerge (
      [
        {
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
