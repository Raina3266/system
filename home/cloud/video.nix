# ~/Videos: on-demand FUSE mount of GoogleDrive:Video (~535 GB).
#
# With --vfs-cache-mode full a file is downloaded into the VFS cache 
# when it is opened, and evicted 30 minutes after its last access. 
# The cache is capped at 10G: without a max size, opening files from a ~535 GB
# remote can grow ~/.cache/rclone without bound on the encrypted /home.
#
# status: systemctl --user status rclone-mount-video.service
# errors: journalctl --user-unit rclone-mount-video.service
{ config, pkgs, ... }:
let
  videoDir = "${config.home.homeDirectory}/Videos";
  # The store's fusermount3 lacks the setuid bit; use the NixOS wrapper.
  fusermount = "/run/wrappers/bin/fusermount3";
in
{
  systemd.user.services.rclone-mount-video = {
    Unit = {
      Description = "Mount GoogleDrive:Video at ~/Videos (on-demand)";
      After = [ "network-online.target" ];
      Wants = [ "network-online.target" ];
    };

    Service = {
      # rclone signals readiness once the mount is live.
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
        # Clear a mount left behind by an unclean shutdown; '-' ignores failure.
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
