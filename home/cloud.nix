{
  config,
  pkgs,
  ...
}:
let
  mountDir_gdrive = "${config.home.homeDirectory}/GoogleDrive";
  musicDir_local = "${config.home.homeDirectory}/Music";
  musicRemote = "GoogleDrive:Music";

  # bisync state directory. Holds the lock file and per-side listings
  # that bisync uses to detect changes since the last successful run.
  bisyncStateDir = "${config.xdg.stateHome}/rclone-bisync-music";

  # Wrapper script for the bisync run. Using a real script (instead of
  # an inline `bash -c '...'` in ExecStart) sidesteps systemd's
  # notoriously finicky ExecStart quoting rules — the script path is a
  # single argument with no embedded quotes to parse.
  bisyncScript = pkgs.writeShellScript "rclone-bisync-music.sh" ''
    set -euo pipefail

    # First-run detection: if no lock file exists yet, do a --resync
    # to establish the baseline. Otherwise run a normal two-way sync.
    #
    # --check-access is a steady-state safety feature: it aborts unless
    # matching RCLONE_TEST files exist on both sides. It is *enforced
    # during --resync too* (per the rclone docs), so it would block the
    # very first resync from ever completing — a deadlock, since the
    # lock file that marks "first run done" is only written by a
    # successful run. We therefore disable --check-access on the first
    # (resync) run, and enable it for every subsequent normal sync.
    if [ ! -f "${bisyncStateDir}/bisync.lck" ]; then
      resync_flag="--resync"
      check_access_flag=""
    else
      resync_flag=""
      check_access_flag="--check-access"
    fi

    exec ${pkgs.rclone}/bin/rclone bisync \
      "${musicDir_local}" "${musicRemote}" \
      --workdir "${bisyncStateDir}" \
      $check_access_flag \
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
in
{
  config = {
    home.packages = with pkgs; [
      fuse3
      rclone
    ];

    # ── FUSE mount for browsing the rest of Google Drive ──────────────
    # Kept for ad-hoc access to non-Music files. The Music subtree is
    # handled separately by rclone-bisync-music below, which keeps a real
    # local copy in ~/Music that survives reboots.
    #
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
        # FUSE filesystems as an unprivileged user. Ignore failure if the
        # mount is already gone (e.g. bisync stopped it via Conflicts=).
        ExecStop = ''
          /run/wrappers/bin/fusermount3 -u '${mountDir_gdrive}' || true
        '';
        Restart = "on-failure";
        RestartSec = 10;
      };

      Install = {
        WantedBy = [ "default.target" ];
      };
    };

    # ── Two-way sync for ~/Music ↔ GoogleDrive:Music ──────────────────
    #
    # Keeps a real local copy of the Music folder on disk (survives
    # reboots, no eviction, no FUSE dependency) and propagates changes
    # both ways: local edits upload, remote changes download.
    #
    # Conflict policy: the newer version wins; the older version is
    # kept with a `-conflict` suffix so nothing is silently lost.
    #
    # IMPORTANT: the very first run must be a --resync to establish the
    # baseline. The ExecStartPre below detects a missing lock file (i.e.
    # bisync has never completed successfully) and runs `--resync` once
    # *without* --check-access (which would otherwise deadlock the first
    # run, since --check-access is enforced during --resync and there are
    # no RCLONE_TEST files yet). Subsequent runs are normal two-way syncs
    # with --check-access enabled for safety.
    #
    # to view status:  systemctl --user status rclone-bisync-music.service
    # to view errors:  journalctl --user-unit rclone-bisync-music.service
    # to force a full re-sync: systemctl --user stop rclone-bisync-music.timer
    #   then: rm -rf ~/.local/state/rclone-bisync-music
    #   then: systemctl --user start rclone-bisync-music.service
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
        # A large library over a slow link can take a while.
        TimeoutStartSec = "2h";

        # Ensure both the local directory and the bisync state directory
        # exist. The state dir holds the lock file; if it's absent on
        # startup, bisync treats this as a first run and the wrapper
        # script adds --resync automatically.
        ExecStartPre = [
          "${coreutils}/bin/mkdir -p '${musicDir_local}'"
          "${coreutils}/bin/mkdir -p '${bisyncStateDir}'"
        ];

        # The wrapper script handles first-run detection (--resync when
        # no lock file exists yet) and conflict resolution (newer wins,
        # loser kept with a `-conflict` suffix).
        ExecStart = "${bisyncScript}";
      };
    };

    # Run the sync shortly after boot (give the network time to come up)
    # and then every hour to pick up changes from either side.
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
  };
}
