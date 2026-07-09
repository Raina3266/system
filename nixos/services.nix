{
  pkgs,
  ...
}:
{
  # List packages installed in system profile. To search, run: $ nix search wget
  environment.systemPackages = with pkgs; [
    vim
    ripgrep
    sushi
    gnomeExtensions.simple-timer
    gnomeExtensions.todotxt
    gnomeExtensions.clipboard-history
    gnomeExtensions.astra-monitor
  ];

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

  programs.nix-ld.enable = true;
  programs.kdeconnect = {
    enable = true;
    package = pkgs.gnomeExtensions.gsconnect;
  };

  services.openssh.enable = true;

  services.postgresql.enable = true;
  services.postgresql.authentication = pkgs.lib.mkForce ''
    local all all           trust
    host  all all 0.0.0.0/0 trust
    host  all all ::0/0     trust
  '';
  services.jellyfin = {
    enable = true;
    openFirewall = true;
    user = "jellyfin";
    group = "jellyfin";
  };

  # Enable Services
  services.accounts-daemon.enable = true;
  services.gnome.gnome-keyring.enable = true;
  services.fprintd.enable = true;
  services.fwupd.enable = true;
  services.printing.enable = true;
  services.gvfs.enable = true;

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

  services.udev.enable = true;
}
