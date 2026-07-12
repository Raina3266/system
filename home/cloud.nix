{
  config,
  pkgs,
  ...
}:
let
  mountDir_gdrive = "${config.home.homeDirectory}/GoogleDrive";
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
        Type = "simple";
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

        # Use the setuid wrapper at /run/wrappers/bin/fusermount3, not the
        # non-setuid copy in the nix store — only the wrapper can unmount
        # FUSE filesystems as an unprivileged user.
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

    # Pre-cache the Music folder by reading every file through the FUSE
    # mount. With --vfs-cache-mode full on the mount, reading a file
    # downloads it into the VFS cache, so subsequent accesses are local.
    # Runs as a oneshot after the mount is up, and on a timer to pick up
    # newly added remote files.
    #
    # Note: the mount's --vfs-cache-max-size is 5G; if Music exceeds that,
    # older cached files will be evicted. Raise the limit in the mount
    # service if needed.
    systemd.user.services.rclone-cache-music = {
      Unit = {
        Description = "Pre-cache Google Drive Music folder into the VFS mount";
        After = [ "rclone-mount-gdrive.service" ];
        Requires = [ "rclone-mount-gdrive.service" ];
      };

      Service = with pkgs; {
        Type = "oneshot";
        Environment = "HOME=%h";
        # Run at low priority, like the mount service.
        Nice = 10;
        IOSchedulingClass = "best-effort";
        IOSchedulingPriority = 7;
        # Reading a large library can take a while on first run.
        TimeoutStartSec = "2h";

        # `cat > /dev/null` reads each file end-to-end through the mount,
        # which is what triggers the VFS download. `-type f` skips dirs.
        ExecStart = ''
          ${findutils}/bin/find '${mountDir_gdrive}/Music' -type f -print0 \
            | xargs -0 -I{} sh -c 'cat "$1" > /dev/null' _ {}
        '';
      };
    };

    systemd.user.timers.rclone-cache-music = {
      Install = {
        WantedBy = [ "timers.target" ];
      };
      Timer = {
        OnBootSec = "2min";
        OnUnitActiveSec = "1h";
        Persistent = true;
      };
    };
  };
}
