{
  pkgs,
  ...
}:
{
  # List packages installed in system profile. To search, run: $ nix search wget
  environment.systemPackages = with pkgs; [
    tailscale
    vim
    ripgrep
    fprintd
    sushi
    yt-dlp
    postgresql_18
    clash-verge-rev
    gnomeExtensions.clipboard-history
    # v4l2-ctl -d /dev/video33 --set-fmt-video=width=1280,height=720,pixelformat=YUYV
    v4l-utils
    webcamoid
    ipu6epmtl-camera-hal
    gst_all_1.icamerasrc-ipu6epmtl
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

  virtualisation.docker.enable = true;

  services.tailscale.enable = true;
  systemd.services.tailscaled.wantedBy = pkgs.lib.mkForce [ ];
  services.tailscale.extraUpFlags = [
    "--accept-dns=true"
  ];

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
  services.udev.extraRules = ''
    ACTION=="add", SUBSYSTEM=="video4linux", ATTR{name}=="Intel MIPI Camera", \
      RUN+="${pkgs.v4l-utils}/bin/v4l2-ctl -d $env{DEVNAME} \
        --set-fmt-video=width=1280,height=720,pixelformat=YUYV \
        --set-parm=30"

    # If the system is not a video device, we skip these rules by jumping to the end
    SUBSYSTEM!="video4linux", GOTO="hide_cam_end"
    # I found its name with udevadm info -q all -a /dev/video0
    # If this is not the dummy video, we also skip these rules.
    ATTR{name}!="Dummy video device (0x0000)", GOTO="hide_cam_end"
    ACTION=="add", RUN+="${pkgs.coreutils}/bin/mkdir -p /dev/not-for-user"
    ACTION=="add", RUN+="${pkgs.coreutils}/bin/mv -f $env{DEVNAME} /dev/not-for-user/"

    ACTION=="remove", RUN+="${pkgs.coreutils}/bin/rm -f /dev/not-for-user/$name"
    ACTION=="remove", RUN+="${pkgs.coreutils}/bin/rm -f /dev/not-for-user/$env{ID_SERIAL}"

    LABEL="hide_cam_end"
  '';
}
