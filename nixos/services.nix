{
  config,
  pkgs,
  ...
}: {
  # List packages installed in system profile. To search, run: $ nix search wget
  environment.systemPackages = with pkgs; [
    vim
    ripgrep
    fprintd
    sushi
    digikam
    ffmpegthumbnailer
    gnome-online-accounts
    gnomeExtensions.pano
    gnomeExtensions.tracker
    gnomeExtensions.task-up-ultralite
    gnomeExtensions.translate-clipboard
  ];

  environment.gnome.excludePackages = with pkgs; [
    epiphany
    geary
    yelp
    seahorse
    totem
    snapshot
    decibels
    # loupe  Image Viewer.
    gnome-photos
    gnome-contacts
    gnome-music
    gnome-maps
    gnome-tour
    gnome-calendar
    gnome-connections
    gnome-console
  ];

  # Enable Services
  services.accounts-daemon.enable = true;
  services.gnome.gnome-keyring.enable = true;
  services.fprintd.enable = true;
  services.udev.enable = true;
  services.fwupd.enable = true;
  services.printing.enable = true;

  # Enable sound with pipewire.
  services.pulseaudio.enable = false;
  security.rtkit.enable = true;
  services.pipewire = {
    enable = true;
    alsa.enable = true;
    alsa.support32Bit = true;
    pulse.enable = true;
    # If you want to use JACK applications, uncomment this
    #jack.enable = true;

    # use the example session manager (no others are packaged yet so this is enabled by default,
    # no need to redefine it in your config for now)
    #media-session.enable = true;
  };
}
