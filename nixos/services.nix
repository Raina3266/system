# Desktop environment, system packages, and services.
#
# Grouped by concern: desktop, sound, desktop daemons, network services,
# media services, database, and odds-and-ends. Core system identity
# (boot, networking, locale, hardware) lives in ./default.nix.
{
  config,
  pkgs,
  ...
}:
let
  webcamSource = "/dev/cam-raw";
  webcamVideoNr = 10;
  webcamCardLabel = "Cropped Webcam";
  packages = import ../packages.nix {
    inherit pkgs;
    kernelPackages = config.boot.kernelPackages;
  };
  inherit (packages) webcamCrop;
in
{
  # Make the packaged supervisor and device settings available to the
  # centralized systemd module without rebuilding the package there.
  _module.args.croppedWebcam = {
    package = webcamCrop;
    source = webcamSource;
    videoNr = webcamVideoNr;
  };

  # ── System packages ───────────────────────────────────────────────────
  environment.systemPackages = with pkgs; [
    vim
    ripgrep
    sushi
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
  services.udev.enable = true;

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
  services.postgresql.authentication = pkgs.lib.mkForce ''
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
