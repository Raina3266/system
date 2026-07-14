# Desktop environment, system packages, and services.
#
# Grouped by concern: desktop, sound, desktop daemons, network services,
# media services, database, and odds-and-ends. Core system identity
# (boot, networking, locale, hardware) lives in ./configuration.nix.
{
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
    simple-scan
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

  xdg.portal = {
    enable = true;
    extraPortals = with pkgs; [ xdg-desktop-portal-gnome ];
  };

  # File manager
  programs.thunar.enable = true;
  programs.xfconf.enable = true;
  programs.thunar.plugins = with pkgs; [
    thunar-volman
    thunar-media-tags-plugin
  ];
  services.tumbler.enable = true; # Thumbnail support for images

  # ── Sound (PipeWire) ──────────────────────────────────────────────────
  services.pulseaudio.enable = false;
  security.rtkit.enable = true;
  services.pipewire = {
    enable = true;
    alsa.enable = true;
    alsa.support32Bit = true;
    pulse.enable = true;
  };

  # ── Desktop daemons ───────────────────────────────────────────────────
  services.accounts-daemon.enable = true;
  services.gnome.gnome-keyring.enable = true;
  services.gvfs.enable = true;
  services.fprintd.enable = true;
  services.fwupd.enable = true;
  services.printing.enable = true;
  services.udev.enable = true;

  # ── Network services ──────────────────────────────────────────────────
  services.openssh.enable = true;

  programs.kdeconnect = {
    enable = true;
    package = pkgs.gnomeExtensions.gsconnect;
  };

  # ── Media services ────────────────────────────────────────────────────
  services.jellyfin = {
    enable = true;
    openFirewall = true;
    user = "jellyfin";
    group = "jellyfin";
  };

  # ── Database ──────────────────────────────────────────────────────────
  services.postgresql.enable = true;
  services.postgresql.authentication = pkgs.lib.mkForce ''
    local all all           trust
    host  all all 0.0.0.0/0 trust
    host  all all ::0/0     trust
  '';

  # ── Misc ──────────────────────────────────────────────────────────────
  programs.nix-ld.enable = true;

  # Cropped virtual webcam (see ./webcam-crop.nix).
  services'.croppedWebcam.enable = true;
}
