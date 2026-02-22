# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).
{
  pkgs,
  inputs,
  lib,
  ...
}:
{
  imports = [
    inputs.home-manager.nixosModules.home-manager
    ./services.nix
  ];

  options.services' = with lib; {
    personal.enable = mkEnableOption "personal stuff";
    work.enable = mkEnableOption "work stuff";
  };

  config = {

    nix.settings.experimental-features = "nix-command flakes";

    # User Account
    users.users.raina = {
      isNormalUser = true;
      description = "Raina";
      extraGroups = [
        "networkmanager"
        "wheel"
        "docker"
        "libvirtd"
      ];
    };

    home-manager.backupFileExtension = "backup";
    home-manager.useUserPackages = true;
    home-manager.users.raina = import ../home;
    home-manager.useGlobalPkgs = true;
    home-manager.extraSpecialArgs = {
      inherit inputs;
    };

    # Desktop Environment
    # Enable the X11 windowing system.
    services.xserver.enable = true;
    services.libinput.enable = true;

    # Enable the GNOME Desktop Environment.
    services.displayManager.gdm.enable = true;
    services.desktopManager.gnome.enable = true;

    # Bootloader.
    boot.loader.systemd-boot.enable = true;
    boot.loader.efi.canTouchEfiVariables = true;

    boot.kernelPackages = pkgs.linuxPackages_latest;

    # Ensure v4l2 modules load
    boot.kernelModules = [
      "v4l2loopback"
      "videobuf2_v4l2"
    ];

    # Enable networking
    networking.networkmanager.enable = true;
    networking.networkmanager.dns = "none";

    # Network
    networking.nameservers = [
      "100.100.100.100"
      "1.1.1.1"
      "9.9.9.9"
    ];
    networking.search = [
      "tail689b6f.ts.net"
      "example.ts.net"
    ];

    # networking.wireless.enable = true;  # Enables wireless support via wpa_supplicant.

    # Time and Language
    # Set your time zone.
    time.timeZone = "Europe/London";

    # Select internationalisation properties.
    i18n.defaultLocale = "en_GB.UTF-8";

    i18n.extraLocaleSettings = {
      LC_ADDRESS = "en_GB.UTF-8";
      LC_IDENTIFICATION = "en_GB.UTF-8";
      LC_MEASUREMENT = "en_GB.UTF-8";
      LC_MONETARY = "en_GB.UTF-8";
      LC_NAME = "en_GB.UTF-8";
      LC_NUMERIC = "en_GB.UTF-8";
      LC_PAPER = "en_GB.UTF-8";
      LC_TELEPHONE = "en_GB.UTF-8";
      LC_TIME = "en_GB.UTF-8";
    };

    # List services that you want to enable:
    xdg.portal = {
      enable = true;
      extraPortals = with pkgs; [ xdg-desktop-portal-gnome ];
    };
    
    # Configure network proxy if necessary
    # networking.proxy.default = "http://user:password@proxy:port/";
    # networking.proxy.noProxy = "127.0.0.1,localhost,internal.domain";

    # Some programs need SUID wrappers, can be configured further or are
    # started in user sessions.
    # programs.mtr.enable = true;
    # programs.gnupg.agent = {
    #   enable = true;
    #   enableSSHSupport = true;
    # };

    # Enable the OpenSSH daemon.
    # services.openssh.enable = true;

    # Open ports in the firewall.
    # networking.firewall.allowedTCPPorts = [ ... ];
    # networking.firewall.allowedUDPPorts = [ ... ];
    # Or disable the firewall altogether.
    # networking.firewall.enable = false;

    hardware.intel-gpu-tools.enable = true;
    hardware.graphics.extraPackages = with pkgs; [
      intel-media-driver
      intel-vaapi-driver
      libvdpau-va-gl
      vpl-gpu-rt
    ];
    environment.sessionVariables = {
      LIBVA_DRIVER_NAME = "iHD";
    }; # Force intel-media-driver
  };
}
