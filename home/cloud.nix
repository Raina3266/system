{ lib, config, pkgs, ... }:

let
  mountDir_gdrive = "${config.home.homeDirectory}/Documents/GoogleDrive";
  mountDir_onedrive_personal = "${config.home.homeDirectory}/Documents/OnedrivePersonal";
  mountDir_onedrive_whu = "${config.home.homeDirectory}/Documents/OnedriveWhu";
  mountDir_onedrive_ucl = "${config.home.homeDirectory}/Documents/OnedriveUcl";
in {
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
      ExecStartPre= "/bin/sh -c 'until ${unixtools.ping}/bin/ping -c1 google.com; do ${coreutils}/bin/sleep 1; done;'";
      ExecStart = ''
        ${rclone}/bin/rclone mount GoogleDrive: '${mountDir_gdrive}' --vfs-cache-mode full --allow-non-empty --uid $(id -u jellyfin) --gid $(id -g jellyfin)
        ${coreutils}/bin/ls ${mountDir_gdrive}
      '';
      
      ExecStop = ''
        ${fuse3}/bin/fusermount3 -u '${mountDir_gdrive}'
      '';
      Restart = "on-failure";
    };

    Install = {
      WantedBy = [ "default.target" ];
    };
  };

  systemd.user.services.rclone-mount-onedrive-personal = {
    Unit = {
      Description = "Mount Onedrive Personal";
      After = [ "network-online.target" ];
      Wants = [ "network-online.target" ];
    };

    Service = with pkgs; {
      ExecStartPre= "/bin/sh -c 'until ${unixtools.ping}/bin/ping -c1 google.com; do ${coreutils}/bin/sleep 1; done;'";
      ExecStart = ''
        ${rclone}/bin/rclone mount OnedrivePersonal: '${mountDir_onedrive_personal}' --vfs-cache-mode full --allow-non-empty
        ${coreutils}/bin/ls ${mountDir_onedrive_personal}
      '';
      
      ExecStop = ''
        ${fuse3}/bin/fusermount3 -u '${mountDir_onedrive_personal}'
      '';
      Restart = "on-failure";
    };

    Install = {
      WantedBy = [ "default.target" ];
    };
  };

  systemd.user.services.rclone-mount-onedrive-whu = {
    Unit = {
      Description = "Mount Onedrive WHU";
      After = [ "network-online.target" ];
      Wants = [ "network-online.target" ];
    };

    Service = with pkgs; {
      ExecStartPre= "/bin/sh -c 'until ${unixtools.ping}/bin/ping -c1 google.com; do ${coreutils}/bin/sleep 1; done;'";
      ExecStart = ''
        ${rclone}/bin/rclone mount OnedriveWhu: '${mountDir_onedrive_whu}' --vfs-cache-mode full --allow-non-empty
        ${coreutils}/bin/ls ${mountDir_onedrive_whu}
      '';
      
      ExecStop = ''
        ${fuse3}/bin/fusermount3 -u '${mountDir_onedrive_whu}'
      '';
      Restart = "on-failure";
    };

    Install = {
      WantedBy = [ "default.target" ];
    };
  };

  systemd.user.services.rclone-mount-onedrive-ucl = {
    Unit = {
      Description = "Mount Onedrive UCL";
      After = [ "network-online.target" ];
      Wants = [ "network-online.target" ];
    };

    Service = with pkgs; {
      ExecStartPre= "/bin/sh -c 'until ${unixtools.ping}/bin/ping -c1 google.com; do ${coreutils}/bin/sleep 1; done;'";
      ExecStart = ''
        ${rclone}/bin/rclone mount OnedriveUcl: '${mountDir_onedrive_ucl}' --vfs-cache-mode full --allow-non-empty 
        ${coreutils}/bin/ls ${mountDir_onedrive_ucl}
      '';
      
      ExecStop = ''
        ${fuse3}/bin/fusermount3 -u '${mountDir_onedrive_ucl}'
      '';
      Restart = "on-failure";
    };

    Install = {
      WantedBy = [ "default.target" ];
    };
  };
}
