{
  config,
  pkgs,
  ...
}:
let
  mountDir_gdrive = "${config.home.homeDirectory}/Documents/GoogleDrive";
  localMusicDir = "${config.home.homeDirectory}/Music";
in
{
  config = {
    home.packages = with pkgs; [
      fuse3
      rclone
    ];

    # to view status, `systemctl --user status rclone-mount-gdrive.service`
    # to view errors, `journalctl --user-unit rclone-mount-gdrive.service`
    systemd.user.services.rclone-mount-gdrive = {
      Unit = {
        Description = "Mount Google Drive";
        After = [ "network-online.target" ];
        Wants = [ "network-online.target" ];
      };

      Service = with pkgs; {
        Type = "notify";
        # Run at low priority so rclone never starves interactive work.
        Nice = 10;
        IOSchedulingClass = "best-effort";
        IOSchedulingPriority = 7;
        # Ensure rclone can find ~/.config/rclone/rclone.conf.
        Environment = "HOME=%h";
        # Give the mount enough time to come up on slow networks.
        TimeoutStartSec = "60s";

        # Ensure the mountpoint exists. Connectivity is handled by
        # After=network-online.target + rclone's own --retries + Restart=on-failure,
        # so no ping loop is needed.
        ExecStartPre = "${coreutils}/bin/mkdir -p '${mountDir_gdrive}'";

        ExecStart = ''
          ${rclone}/bin/rclone mount GoogleDrive: '${mountDir_gdrive}' \
            --systemd \
            --vfs-cache-mode full \
            --vfs-cache-max-size 5G \
            --vfs-cache-max-age 168h \
            --vfs-cache-poll-interval 5m \
            --vfs-read-chunk-size 8M \
            --vfs-read-chunk-size-limit 256M \
            --dir-cache-time 24h \
            --poll-interval 1m \
            --buffer-size 16M \
            --low-level-retries 10 \
            --retries 3 \
            --timeout 30s \
            --contimeout 30s \
            --tpslimit 10 \
            --tpslimit-burst 20
        '';

        ExecStop = ''
          /run/wrappers/bin/fusermount3 -u '${mountDir_gdrive}'
        '';
        Restart = "on-failure";
        RestartSec = 10;
      };

      Install = {
        WantedBy = [ "default.target" ];
      };
    };

    # ---------------------------------------------------------------------------
    # Local mirror of GoogleDrive:Music -> ~/Music
    # Run once manually to seed the initial copy: rclone sync GoogleDrive:Music ~/Music --progress
    #
    # Status:  systemctl --user status rclone-sync-music.service
    # Trigger: systemctl --user start rclone-sync-music.service
    # Timer:   systemctl --user list-timers rclone-sync-music.timer
    # ---------------------------------------------------------------------------
    systemd.user.services.rclone-sync-music = {
      Unit = {
        Description = "Sync Google Drive Music folder to local disk";
        After = [ "network-online.target" ];
        Wants = [ "network-online.target" ];
      };

      Service = with pkgs; {
        Type = "oneshot";
        # Low priority so sync never competes with interactive work.
        Nice = 10;
        IOSchedulingClass = "best-effort";
        IOSchedulingPriority = 7;
        # Ensure rclone can find ~/.config/rclone/rclone.conf.
        Environment = "HOME=%h";
        # A real sync can take much longer than systemd's 90s default.
        TimeoutStartSec = "10min";

        ExecStart = ''
          ${rclone}/bin/rclone sync GoogleDrive:Music '${localMusicDir}' \
            --transfers 4 \
            --checkers 8 \
            --retries 3 \
            --low-level-retries 10 \
            --timeout 30s \
            --contimeout 30s \
            --tpslimit 20 \
            --tpslimit-burst 40 \
            --bwlimit 10M \
            --update \
            --modify-window 1s \
            --skip-links \
            --create-empty-src-dirs \
            --log-level INFO
        '';
      };
    };

    systemd.user.timers.rclone-sync-music = {
      Unit = {
        Description = "Periodically sync Google Drive Music folder to local disk";
      };

      Timer = {
        # First run 2 minutes after login (gives the mount/network time to settle).
        OnBootSec = "2min";
        # Then every 30 minutes while logged in.
        OnUnitActiveSec = "30min";
        # Catch up if a scheduled run was missed (e.g. laptop was asleep).
        Persistent = true;
        # Don't pile up missed runs; just run once on wake.
        AccuracySec = "1min";
      };

      Install = {
        WantedBy = [ "timers.target" ];
      };
    };
  };
}
