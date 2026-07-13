# Edit this configuration file to define what should be installed on
# your system.  Help is available in the configuration.nix(5) man page
# and in the NixOS manual (accessible by running ‘nixos-help’).
#
# This file holds the core system identity: nix settings, the user account,
# boot, filesystems, networking, time/locale, and hardware. Services, the
# desktop environment, and installed packages live in ./services.nix.
{
  pkgs,
  inputs,
  ...
}:
{
  imports = [
    inputs.home-manager.nixosModules.home-manager
    ./services.nix
  ];

  config = {
    # Nix
    nix.settings.experimental-features = "nix-command flakes";
    nix.settings.trusted-users = [
      "root"
      "raina"
      "@wheel"
    ];

    # User account & home-manager
    users.users.raina = {
      isNormalUser = true;
      description = "Raina";
      extraGroups = [
        "networkmanager"
        "wheel"
        "input"
      ];
    };

    home-manager = {
      backupFileExtension = "backup";
      useUserPackages = true;
      useGlobalPkgs = true;
      users.raina = import ../home;
      extraSpecialArgs = {
        inherit inputs;
      };
    };

    # Boot
    boot.loader.systemd-boot.enable = true;
    boot.loader.efi.canTouchEfiVariables = true;
    boot.kernelPackages = pkgs.linuxPackages_latest;

    # File systems & swap
    swapDevices = [
      {
        device = "/swap/swapfile";
        size = 26 * 1024;
      }
    ];

    services.btrfs.autoScrub = {
      enable = true;
      fileSystems = [ "/" ];
      interval = "weekly";
    };

    # Networking
    networking.hostName = "raina";
    networking.networkmanager.enable = true;
    networking.networkmanager.dns = "none";
    networking.nameservers = [
      "8.8.8.8"
      "100.100.100.100"
      "1.1.1.1"
      "9.9.9.9"
    ];
    networking.firewall = rec {
      allowedTCPPortRanges = [
        {
          from = 1714;
          to = 1764;
        }
      ];
      allowedUDPPortRanges = allowedTCPPortRanges;
    };

    # Time & locale
    time.timeZone = "Europe/London";

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

    # Input method (CJK)
    i18n.inputMethod = {
      enable = true;
      type = "ibus";
      ibus.engines = with pkgs.ibus-engines; [ libpinyin ];
    };

    # Hardware (Intel graphics / video acceleration)
    hardware.intel-gpu-tools.enable = true;
    hardware.graphics.extraPackages = with pkgs; [
      intel-media-driver
      intel-vaapi-driver
      libvdpau-va-gl
      vpl-gpu-rt
    ];
    environment.sessionVariables = {
      LIBVA_DRIVER_NAME = "iHD"; # Force intel-media-driver
    };

    # Fonts
    fonts = {
      enableDefaultPackages = true;
      packages = with pkgs; [
        noto-fonts
        noto-fonts-cjk-sans
        noto-fonts-cjk-serif
        wqy_zenhei
        wqy_microhei

        nerd-fonts.jetbrains-mono
        nerd-fonts.symbols-only
      ];

      # Make a Nerd Font the system default so apps that use the
      # fontconfig default sans/serif/monospace (niri's hotkey overlay,
      # tab titles, etc.) pick up Nerd Font glyphs for icons.
      fontconfig = {
        enable = true;
        defaultFonts = {
          sansSerif = [ "JetBrainsMono Nerd Font" "Symbols Nerd Font Mono" "Noto Sans" "Noto Sans CJK SC" "Noto Sans CJK JP" ];
          serif = [ "JetBrainsMono Nerd Font" "Symbols Nerd Font Mono" "Noto Serif" "Noto Serif CJK SC" "Noto Serif CJK JP" ];
          monospace = [ "JetBrainsMono Nerd Font" "Symbols Nerd Font Mono" "Noto Sans Mono CJK SC" ];
          emoji = [ "Symbols Nerd Font Mono" "Noto Color Emoji" ];
        };
      };
    };

    # This value determines the NixOS release from which the default
    # settings for stateful data, like file locations and database versions
    # on your system were taken. It‘s perfectly fine and recommended to leave
    # this value at the release version of the first install of this system.
    # Before changing this value read the documentation for this option
    # (e.g. man configuration.nix or on https://nixos.org/nixos/options.html).
    system.stateVersion = "26.05"; # Did you read the comment?
  };
}
