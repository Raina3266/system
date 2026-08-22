# System-level units and service orchestration.
{
  croppedWebcam,
  lib,
  ...
}:
{
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
    cropped-webcam = {
      description = "Cropped virtual webcam supervisor (${croppedWebcam.source} -> /dev/video${toString croppedWebcam.videoNr})";
      wantedBy = [ "multi-user.target" ];
      after = [ "systemd-udev-settle.service" ];
      serviceConfig = {
        ExecStart = "${croppedWebcam.package}/bin/webcam-crop --source ${croppedWebcam.source} --output /dev/video${toString croppedWebcam.videoNr}";
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
