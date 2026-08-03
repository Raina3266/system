# mkBisync generates every systemd unit needed to keep a local directory and an
# rclone remote in sync: the bisync service and its 30-minute timer, a watcher
# that pushes local edits within `debounce` seconds, and a trash-cleanup
# service with its daily timer.
{
  config,
  pkgs,
  lib,
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

      # rclone requires each backup dir to be a non-overlapping path on the
      # same remote as the side it backs up: --backup-dir1 mirrors Path1 (local).
      # Remote-side overwrites fall back to Drive's own trash.
      trashLocal = "${config.xdg.dataHome}/${unit}/trash";

      rclone = "${pkgs.rclone}/bin/rclone";
      mkdir = "${pkgs.coreutils}/bin/mkdir -p";

      flags = [
        "--backup-dir1 '${trashLocal}'"
      ]
      ++ map (p: "--exclude '${p}'") excludes;

      # The same excludes, as one POSIX extended regex of path prefixes, so the
      # watcher does not wake on folders bisync would ignore anyway. rclone's
      # "Video/**" becomes "^/home/raina/GoogleDrive/Video/".
      excludeRegex = lib.concatMapStringsSep "|" (
        p: "^" + builtins.replaceStrings [ "." ] [ "\\." ] "${localDir}/${lib.removeSuffix "/**" p}" + "/"
      ) excludes;

      # Built as a list and joined, so an empty `excludes` cannot leave a
      # dangling backslash continuation and swallow the following flag.
      watchFlags = [
        "--monitor --recursive --quiet"
        "--event close_write,create,delete,move"
        "--format '%w%f'"
      ]
      ++ lib.optional (excludes != [ ]) "--excludei '${excludeRegex}'";

      # rclone writes *.lst-new during a run and renames to *.lst only on
      # success, so the presence of a *.lst is what marks "already resynced".
      # Matching *.lst-new here would be wrong: an interrupted --resync leaves
      # those behind, and treating them as state makes every later run a
      # (doomed) incremental one. `find` is used rather than a glob so a
      # stale *.lst-new cannot satisfy the test.
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
          --suffix "-$(${pkgs.coreutils}/bin/date +%Y-%m-%dT%H%M%S)" \
          --suffix-keep-extension \
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

      # Local -> remote push. bisync has no incremental mode (every run re-lists
      # both sides, including a Drive API call), so the debounce is what keeps
      # this cheap: a burst of saves collapses into one run once things go quiet.
      #
      # `systemctl start` on a queued oneshot collapses duplicates and waits for
      # any in-flight run, so this can never overlap the timer's run.
      watchScript = pkgs.writeShellScript "${unit}-watch.sh" ''
        set -euo pipefail

        ${pkgs.inotify-tools}/bin/inotifywait \
          ${lib.concatStringsSep " \\\n  " watchFlags} \
          '${localDir}' \
        | while read -r _changed; do
            # Drain the rest of the burst: keep resetting the timer until the
            # tree has been quiet for the full debounce window.
            while read -r -t ${toString debounce} _more; do :; done
            ${pkgs.systemd}/bin/systemctl --user start --no-block '${unit}.service'
          done
      '';

      cleanupScript = pkgs.writeShellScript "${unit}-cleanup.sh" ''
        set -euo pipefail
        ${pkgs.findutils}/bin/find '${trashLocal}' -mindepth 1 \( -type f -o -type l \) -mtime +30 -delete
        ${pkgs.findutils}/bin/find '${trashLocal}' -mindepth 1 -type d -empty -delete
      '';

      # Nice/IOScheduling* are kept for the CPU side and for hosts using BFQ,
      # but ionice is a no-op under the `none` scheduler that NVMe uses, and the
      # `io` cgroup controller is not delegated into the user manager. MemoryHigh
      # is the one throttle that does bite here: it caps how much page cache a
      # listing run can claim, so a sync cannot evict the working set of
      # everything else. It throttles rather than OOM-kills, so a large tree
      # still syncs, just without taking the cache with it.
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
          # Home Manager activation would otherwise SIGTERM an in-flight run.
          # A --resync cannot resume ("just run it again"), so rebuilding more
          # often than a resync takes to finish means it never converges: no
          # *.lst is ever written and every run re-lists the whole tree. Let a
          # running sync finish; the new definition applies on the next start.
          X-SwitchMethod = "keep-old";
        };
        Service = baseService // {
          TimeoutStartSec = timeout;
          ExecStartPre = [
            "${mkdir} '${localDir}'"
            "${mkdir} '${stateDir}'"
            "${mkdir} '${trashLocal}'"
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

      # Catches local edits between timer runs. Remote edits still wait for the
      # timer: there is no inotify for a remote, so pull stays polling-only.
      services."${unit}-watch" = {
        Unit = {
          Description = "Watch ${localDir} and push changes to ${remote}";
          # Start watching only after the first sync, so the initial --resync
          # does not trip the watcher into queueing a redundant second run.
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

      services."${unit}-cleanup" = {
        Unit.Description = "Purge ${name} bisync local trash older than 30 days";
        Service = baseService // {
          TimeoutStartSec = "30m";
          ExecStartPre = "${mkdir} '${trashLocal}'";
          ExecStart = "${cleanupScript}";
        };
      };

      timers."${unit}-cleanup" = {
        Unit.Description = "Daily purge of ${name} bisync trash";
        Timer = {
          OnBootSec = "10min";
          OnUnitActiveSec = "1d";
          Persistent = true;
        };
        Install.WantedBy = [ "timers.target" ];
      };
    };

  syncs = map mkBisync (import ./bisync.nix { inherit config; });
in
{
  imports = [ ./video.nix ];

  home.packages = with pkgs; [
    fuse3
    rclone
  ];

  systemd.user = {
    services = lib.mkMerge (map (s: s.services) syncs);
    timers = lib.mkMerge (map (s: s.timers) syncs);
  };
}
