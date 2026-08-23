# Desktop environment, system packages, services, and the system units that
# belong to them.
#
# Grouped by concern: desktop, sound, desktop daemons, network services,
# media services, webcam, database, and odds-and-ends. Core system identity
# (boot, networking, locale, hardware) lives in ./default.nix.
{
  config,
  lib,
  pkgs,
  repoPackages,
  ...
}:
let
  webcamSource = "/dev/cam-raw";
  webcamVideoNr = 10;
  webcamCardLabel = "Cropped Webcam";
  webcamDevice = "/dev/video${toString webcamVideoNr}";
  inherit (repoPackages) webcamCrop;
in
{
  # ── System packages ───────────────────────────────────────────────────
  environment.systemPackages = with pkgs; [
    vim
    ripgrep
    ffmpegthumbnailer
    gdk-pixbuf
    system-config-printer
    v4l-utils
    webcamCrop
    gnomeExtensions.simple-timer
    gnomeExtensions.clipboard-history
    gnomeExtensions.astra-monitor
  ];

  # ── Desktop environment ───────────────────────────────────────────────
  services.xserver.enable = true;
  services.xserver.xkb = {
    layout = "gb";
    variant = "";
  };
  services.libinput.enable = true;

  services.displayManager.gdm.enable = true;
  services.desktopManager.gnome.enable = true;

  environment.gnome.excludePackages = with pkgs; [
    epiphany
    geary
    yelp
    seahorse
    totem
    snapshot
    decibels
    papers
    showtime
    nautilus
    # loupe  Image Viewer.
    gnome-weather
    gnome-calculator
    gnome-text-editor
    gnome-photos
    gnome-contacts
    gnome-music
    gnome-maps
    gnome-tour
    gnome-calendar
    gnome-connections
    gnome-console
  ];

  programs.niri.enable = true;

  # Power management for waybar's power-profiles-daemon module.
  services.power-profiles-daemon.enable = true;

  # Intel thermal daemon: uses this CPU's DPTF/RAPL sensors to manage
  # thermal limits actively, rather than leaving the firmware to clamp
  # hard to the efficiency floor. Complements (does not replace)
  # power-profiles-daemon, which sets the EPP/platform profile.
  services.thermald.enable = true;

  xdg.portal = {
    enable = true;
    extraPortals = with pkgs; [ xdg-desktop-portal-gtk ];
  };

  # ── Network services ──────────────────────────────────────────────────
  services.openssh.enable = true;

  # ── Sound (PipeWire) ──────────────────────────────────────────────────
  services.pulseaudio.enable = false;
  security.rtkit.enable = true;
  services.pipewire = {
    enable = true;
    alsa.enable = true;
    alsa.support32Bit = true;
    pulse.enable = true;
    wireplumber.enable = true;
  };

  # ── Desktop daemons ───────────────────────────────────────────────────
  services.accounts-daemon.enable = true;
  services.gnome.gnome-keyring.enable = true;
  services.gvfs.enable = true;
  services.fprintd.enable = true;
  services.fwupd.enable = true;

  # ── Printing services ──────────────────────────────────────────────────
  services.printing = {
    enable = true;
    drivers = with pkgs; [
      cups-filters
      cups-browsed
      cnijfilter2
    ];
  };
  services.avahi = {
    enable = true;
    nssmdns4 = true;
    openFirewall = true;
  };
  services.udev.packages = with pkgs; [
    sane-airscan
  ];
  services.ipp-usb.enable = true;

  # ── Media services ────────────────────────────────────────────────────
  # Installed and configured but NOT started at boot
  services.jellyfin = {
    enable = true;
    openFirewall = true;
    user = "jellyfin";
    group = "jellyfin";
  };

  services.immich = {
    enable = true;
    openFirewall = true;
  };

  services.sunshine = {
    enable = true;
    autoStart = false;
    openFirewall = true;
    capSysAdmin = true;
  };

  # Immich and Jellyfin are installed, but start together only on demand.
  systemd.targets.media-stack = {
    description = "On-demand media services (Immich + Jellyfin)";
    unitConfig.StopWhenUnneeded = true;
  };

  # Unit overrides for the services configured above, plus the webcam
  # supervisor whose device settings are defined in this file.
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
      description = "Cropped virtual webcam supervisor (${webcamSource} -> ${webcamDevice})";
      wantedBy = [ "multi-user.target" ];
      after = [ "systemd-udev-settle.service" ];
      serviceConfig = {
        ExecStart = "${webcamCrop}/bin/webcam-crop --source ${webcamSource} --output ${webcamDevice}";
        Restart = "always";
        RestartSec = 2;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        NoNewPrivileges = true;
      };
    };
  };

  # The real camera has no zoom control, so expose only a centre-cropped
  # v4l2loopback camera to desktop applications. The Rust supervisor keeps a
  # placeholder producer attached while idle and powers on the real camera
  # only while an application is consuming the virtual device.
  boot.extraModulePackages = [ config.boot.kernelPackages.v4l2loopback ];
  boot.kernelModules = [ "v4l2loopback" ];
  boot.extraModprobeConfig = ''
    options v4l2loopback devices=1 video_nr=${toString webcamVideoNr} card_label="${webcamCardLabel}" exclusive_caps=1 max_buffers=2
  '';

  services.udev.extraRules = ''
    # Stable path to the real RGB sensor for the cropper service.
    SUBSYSTEM=="video4linux", ATTR{name}=="Integrated Camera: Integrated C", ATTR{index}=="0", SYMLINK+="cam-raw"

    # Keep the physical RGB, IR, and metadata nodes away from user apps. The
    # service runs as root; raina is not in the video group.
    SUBSYSTEM=="video4linux", ATTR{name}=="Integrated Camera: Integrated C", TAG-="uaccess"
    SUBSYSTEM=="video4linux", ATTR{name}=="Integrated Camera: Integrated I", TAG-="uaccess"

    # Friendly alias for the cropped virtual camera.
    SUBSYSTEM=="video4linux", ATTR{name}=="${webcamCardLabel}", SYMLINK+="cam-cropped"
  '';

  # ── Database ──────────────────────────────────────────────────────────
  # Local Unix-socket access for the owner, loopback TCP for apps that
  # connect via 127.0.0.1. No access from other hosts.
  services.postgresql.enable = true;
  services.postgresql.authentication = lib.mkForce ''
    local all all                      peer
    host  all all 127.0.0.1/32         scram-sha-256
    host  all all ::1/128              scram-sha-256
  '';
  services.postgresql.ensureDatabases = [ "raina" ];
  services.postgresql.ensureUsers = [
    {
      name = "raina";
      ensureDBOwnership = true;
    }
  ];

  # ── Misc ──────────────────────────────────────────────────────────────
  programs.nix-ld.enable = true;
}
