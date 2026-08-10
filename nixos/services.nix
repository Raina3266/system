# Desktop environment, system packages, and services.
#
# Grouped by concern: desktop, sound, desktop daemons, network services,
# media services, database, and odds-and-ends. Core system identity
# (boot, networking, locale, hardware) lives in ./configuration.nix.
{
  lib,
  pkgs,
  ...
}:
{
  # ── System packages ───────────────────────────────────────────────────
  environment.systemPackages = with pkgs; [
    vim
    ripgrep
    sushi
    ffmpegthumbnailer
    gdk-pixbuf
    system-config-printer
    nemo-with-extensions
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

  # ── Network services ──────────────────────────────────────────────────
  services.openssh.enable = true;

  programs.kdeconnect = {
    enable = true;
    package = pkgs.gnomeExtensions.gsconnect;
  };

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

  # immich-server keeps Requires=postgresql.target, so Postgres is
  # pulled in automatically when Immich starts.
  #
  # Grouped under a custom target so they can be started/stopped together:
  #   sudo systemctl start media-stack.target
  #   sudo systemctl stop media-stack.target
  #   systemctl list-dependencies media-stack.target
  systemd.targets.media-stack = {
    description = "On-demand media services (Immich + Jellyfin)";
    unitConfig.StopWhenUnneeded = true;
  };

  systemd.services.jellyfin = {
    wantedBy = lib.mkForce [ ];
    partOf = [ "media-stack.target" ];
  };
  systemd.services.immich-server = {
    wantedBy = lib.mkForce [ ];
    partOf = [ "media-stack.target" ];
  };
  systemd.services.immich-machine-learning = {
    wantedBy = lib.mkForce [ ];
    partOf = [ "media-stack.target" ];
  };
  systemd.services.redis-immich = {
    wantedBy = lib.mkForce [ ];
    partOf = [ "media-stack.target" ];
  };

  services.sunshine = {
    enable = true;
    autoStart = false;
    openFirewall = true;
    capSysAdmin = true;
  };

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

  # Cropped virtual webcam (see ./webcam-crop.nix).
  services'.croppedWebcam.enable = true;
}
