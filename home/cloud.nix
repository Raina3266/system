{
  config,
  pkgs,
  ...
}:
let
  mountDir = "${config.home.homeDirectory}/GoogleDrive";
  musicDir = "${config.home.homeDirectory}/Music";
  musicRemote = "GoogleDrive:Music";
  bisyncStateDir = "${config.xdg.stateHome}/rclone-bisync-music";

  # Soft delete: files that lose a sync (deleted or overwritten on the other
  # side) are moved here instead of being removed. Both dirs are local, so
  # recovery needs no download and cleanup works offline. Purged after 30
  # days by the rclone-bisync-music-cleanup service.
  trashDir = "${config.xdg.dataHome}/rclone-bisync-music/trash";
  backupDir_local = "${trashDir}/local";
  backupDir_remote = "${trashDir}/remote";

  bisyncScript = pkgs.writeShellScript "rclone-bisync-music.sh" ''
    set -euo pipefail

    # No --check-access on the first run: RCLONE_TEST files only exist after
    # a successful sync.
    if [ ! -f "${bisyncStateDir}/bisync.lck" ]; then
      resync_flag="--resync"
      check_access_flag=""
    else
      resync_flag=""
      check_access_flag="--check-access"
    fi

    # Timestamped suffix so repeated backups of a same-named file don't
    # overwrite each other in the trash.
    backup_suffix="-$(${pkgs.coreutils}/bin/date +%Y-%m-%dT%H%M%S)"

    exec ${pkgs.rclone}/bin/rclone bisync \
      "${musicDir}" "${musicRemote}" \
      --workdir "${bisyncStateDir}" \
      $check_access_flag \
      --backup-dir1 "${backupDir_local}" \
      --backup-dir2 "${backupDir_remote}" \
      --suffix "$backup_suffix" \
      --suffix-keep-extension \
      --conflict-resolve newer \
      --conflict-suffix conflict \
      --resilient \
      --retries 3 \
      --low-level-retries 10 \
      --timeout 30s \
      --contimeout 30s \
      --tpslimit 10 \
      --tpslimit-burst 20 \
      $resync_flag
  '';

  cleanupScript = pkgs.writeShellScript "rclone-bisync-music-cleanup.sh" ''
    set -euo pipefail

    ${pkgs.findutils}/bin/find "${trashDir}" -mindepth 1 \( -type f -o -type l \) -mtime +30 -delete
    ${pkgs.findutils}/bin/find "${trashDir}" -mindepth 1 -type d -empty -delete
  '';
in
{
  config = {
    home.packages = with pkgs; [
      fuse3
      rclone
    ];

    # FUSE mount of Google Drive, started on first access via the automount
    # unit (no idle daemon). 1G write cache kept for 24h. The unit name
    # must match the mount path (systemd requirement for automount pairs).
    #
    # status: systemctl --user status home-raina-GoogleDrive.service
    # errors: journalctl --user-unit home-raina-GoogleDrive.service
    systemd.user.services."home-raina-GoogleDrive" = {
      Unit = {
        Description = "Mount Google Drive";
        After = [ "network-online.target" ];
        Wants = [ "network-online.target" ];
      };

      Service = with pkgs; {
        Type = "simple";
        Nice = 10;
        IOSchedulingClass = "best-effort";
        IOSchedulingPriority = 7;
        Environment = "HOME=%h";
        TimeoutStartSec = "60s";

        ExecStartPre = "${coreutils}/bin/mkdir -p '${mountDir}'";

        ExecStart = ''
          ${rclone}/bin/rclone mount GoogleDrive: '${mountDir}' \
            --vfs-cache-mode writes \
            --vfs-cache-max-size 1G \
            --vfs-cache-max-age 24h \
            --vfs-read-chunk-size 8M \
            --vfs-read-chunk-size-limit 256M \
            --dir-cache-time 1h \
            --poll-interval 10m \
            --buffer-size 16M \
            --low-level-retries 10 \
            --retries 3 \
            --timeout 30s \
            --contimeout 30s \
            --tpslimit 10 \
            --tpslimit-burst 20
        '';

        # Setuid wrapper: the store fusermount3 lacks permissions.
        ExecStop = ''
          /run/wrappers/bin/fusermount3 -u '${mountDir}' || true
        '';
      };

      Install = {
        WantedBy = [ "default.target" ];
      };
    };

    # Stop the mount after 10 minutes idle; the automount restarts it on
    # the next access to ~/GoogleDrive.
    systemd.user.automounts."home-raina-GoogleDrive" = {
      Unit.Description = "Automount Google Drive";
      Automount = {
        Where = mountDir;
        TimeoutIdleSec = "10min";
      };
      Install.WantedBy = [ "default.target" ];
    };

    # Two-way sync ~/Music <-> GoogleDrive:Music. Newer wins on conflicts;
    # the loser is kept with a -conflict suffix.
    #
    # status:       systemctl --user status rclone-bisync-music.service
    # force resync: rm -rf ~/.local/state/rclone-bisync-music
    #               && systemctl --user start rclone-bisync-music.service
    systemd.user.services.rclone-bisync-music = {
      Unit = {
        Description = "Two-way sync ~/Music with GoogleDrive:Music";
        After = [ "network-online.target" ];
        Wants = [ "network-online.target" ];
      };

      Service = with pkgs; {
        Type = "oneshot";
        Environment = "HOME=%h";
        Nice = 10;
        IOSchedulingClass = "best-effort";
        IOSchedulingPriority = 7;
        TimeoutStartSec = "2h";

        ExecStartPre = [
          "${coreutils}/bin/mkdir -p '${musicDir}'"
          "${coreutils}/bin/mkdir -p '${bisyncStateDir}'"
          "${coreutils}/bin/mkdir -p '${backupDir_local}'"
          "${coreutils}/bin/mkdir -p '${backupDir_remote}'"
        ];

        ExecStart = "${bisyncScript}";
      };
    };

    systemd.user.timers.rclone-bisync-music = {
      Install = {
        WantedBy = [ "timers.target" ];
      };
      Timer = {
        OnBootSec = "1min";
        OnUnitActiveSec = "1h";
        Persistent = true;
      };
    };

    # Purge files older than 30 days from the bisync trash.
    #
    # status: systemctl --user status rclone-bisync-music-cleanup.service
    systemd.user.services.rclone-bisync-music-cleanup = {
      Unit.Description = "Purge bisync trash files older than 30 days";

      Service = {
        Type = "oneshot";
        Environment = "HOME=%h";
        Nice = 10;
        IOSchedulingClass = "best-effort";
        IOSchedulingPriority = 7;
        TimeoutStartSec = "30m";

        ExecStartPre = "${pkgs.coreutils}/bin/mkdir -p '${backupDir_local}' '${backupDir_remote}'";
        ExecStart = "${cleanupScript}";
      };
    };

    systemd.user.timers.rclone-bisync-music-cleanup = {
      Install = {
        WantedBy = [ "timers.target" ];
      };
      Timer = {
        OnBootSec = "5min";
        OnUnitActiveSec = "1d";
        Persistent = true;
      };
    };
  };
}
