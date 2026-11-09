# Core system identity: nix settings, the user account, boot, filesystems,
# networking, time/locale and hardware. Services, the desktop environment and
# installed packages live in ./services.nix.
{
  pkgs,
  inputs,
  ...
}:
let
  kernelPackages = pkgs.linuxPackages_latest;
  system = pkgs.stdenv.hostPlatform.system;
  # One canonical checkout path for every live-linked desktop configuration.
  repoRoot = "/home/raina/System";
  repoPackages = import ../scripts/packages.nix {
    inherit pkgs kernelPackages repoRoot;
    craneLib = inputs.crane.mkLib pkgs;
    agsPackage = inputs.ags.packages.${system}.ags;
    astalPackages = inputs.astal.packages.${system};
    niriWindowButtonsSrc = inputs.niri-window-buttons;
  };
in
{
  imports = [
    inputs.home-manager.nixosModules.home-manager
    ./hardware.nix
    ./services.nix
  ];

  config = {
    _module.args.repoPackages = repoPackages;

    # Browser notifications go to the native Astal notification server.
    environment.etc."opt/chrome/policies/managed/desktop-notifications.json".text =
      builtins.toJSON { AllowSystemNotifications = true; };

    nixpkgs.config.allowUnfree = true;
    nixpkgs.overlays = [
      inputs.nixGL.overlay
      (final: prev:
        let
          # The Rust setup hook requires system AWS-LC, but these recipes omit it.
          withAwsLc = package: package.overrideAttrs (old: {
            buildInputs = (old.buildInputs or [ ]) ++ [ final.aws-lc ];
          });
        in
        {
          mprisence = withAwsLc prev.mprisence;
          clash-verge-rev = prev.clash-verge-rev.override {
            callPackage = path: args:
              let package = final.callPackage path args;
              in if (package.pname or "") == "clash-verge-rev-unwrapped"
                then withAwsLc package
                else package;
          };
        })
      (final: prev: {
        anki = prev.anki.overrideAttrs (old: {
          # The Rust setup hook requires system zstd and SQLite instead of vendored copies.
          nativeBuildInputs = (old.nativeBuildInputs or [ ]) ++ [ final.pkg-config ];
          buildInputs = (old.buildInputs or [ ]) ++ [ final.zstd final.sqlite ];
          env = old.env // {
            # Hatchling needs tomlkit in uv's offline build-isolation wheel pool.
            UV_FIND_LINKS = final.symlinkJoin {
              name = "anki-uv-wheels";
              paths = [
                old.env.UV_FIND_LINKS
                prev.anki.python.pkgs.tomlkit.dist
              ];
            };
          };
        });
      })
    ];

    # Nix
    nix.settings.experimental-features = [
      "nix-command"
      "flakes"
    ];
    nix.settings.trusted-users = [
      "root"
      "raina"
      "@wheel"
    ];

    security.sudo.extraConfig = "Defaults timestamp_timeout=20";

    security.pki.caCertificateBlacklist = [
      "Comodo AAA Services root"
    ];

    # User account & home-manager
    users.users.raina = {
      isNormalUser = true;
      description = "Raina";
      extraGroups = [
        "networkmanager"
        "wheel"
        "input"
        "lp"
        "scanner"
      ];
    };

    home-manager = {
      backupFileExtension = "backup";
      useUserPackages = true;
      useGlobalPkgs = true;
      users.raina = import ../home;
      extraSpecialArgs = {
        inherit inputs repoPackages repoRoot;
      };
    };

    # Boot
    boot.loader.systemd-boot.enable = true;
    boot.loader.efi.canTouchEfiVariables = true;
    boot.kernelPackages = kernelPackages;
    boot.kernel.sysctl = {
      "fs.inotify.max_user_watches" = 524288;
      "fs.inotify.max_user_instances" = 512;
      # Favor reclaiming file cache over paging out interactive application state.
      "vm.swappiness" = 20;
    };

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
    # Use network-provided DNS so captive-portal hostnames can resolve before sign-in.
    networking.networkmanager.dns = "default";
    networking.wireless.iwd.enable = true;
    networking.networkmanager.wifi.backend = "iwd";

    networking.firewall = rec {
      allowedTCPPortRanges = [
        {
          from = 1714;
          to = 1764;
        }
      ];
      allowedUDPPortRanges = allowedTCPPortRanges;
      allowedTCPPorts = [
        631
        9100
      ]; # CUPS (631) & AppSocket/RAW (9100)
      allowedUDPPorts = [
        631
        5353
      ]; # IPP & mDNS/Avahi discovery
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

    # Initrd/recovery console keymap (used for the LUKS passphrase prompt).
    console.keyMap = "uk";

    # Input methods: Chinese (pinyin) and Korean (hangul).
    i18n.inputMethod = {
      enable = true;
      type = "fcitx5";
      fcitx5 = {
        waylandFrontend = true; # if you're on Wayland
        addons = with pkgs; [
          fcitx5-gtk
          qt6Packages.fcitx5-chinese-addons
          fcitx5-hangul
        ];
      };
    };

    # Hardware (Intel graphics / video acceleration)
    hardware.intel-gpu-tools.enable = true;
    hardware.bluetooth.enable = true;

    hardware.graphics.extraPackages = with pkgs; [
      intel-media-driver
      libvdpau-va-gl
      vpl-gpu-rt
    ];
    hardware.sane = {
      enable = true;
      extraBackends = with pkgs; [
        sane-airscan
      ];
    };

    environment.sessionVariables = {
      LIBVA_DRIVER_NAME = "iHD";
      TERMINAL = "ghostty";
      ELECTRON_OZONE_PLATFORM_HINT = "wayland";
      QT_QPA_PLATFORM = "wayland;xcb";
      QT_SCALE_FACTOR_ROUNDING_POLICY = "PassThrough";
    };

    # Fonts
    fonts = {
      enableDefaultPackages = true;
      packages = with pkgs; [
        newcomputermodern
        font-awesome
        noto-fonts
        noto-fonts-cjk-sans
        noto-fonts-cjk-serif
        nerd-fonts.jetbrains-mono
        nerd-fonts.symbols-only
      ];

      # Use proportional fonts for GUI text, with Nerd Font fallbacks for icons.
      # Keep JetBrains Mono as the default for terminals and other monospace text.
      fontconfig = {
        enable = true;
        defaultFonts = {
          sansSerif = [
            "Noto Sans"
            "JetBrainsMono Nerd Font"
            "Symbols Nerd Font Mono"
            "Noto Sans CJK SC"
            "Noto Sans CJK JP"
            "Noto Sans CJK KR"
          ];
          serif = [
            "Noto Serif"
            "JetBrainsMono Nerd Font"
            "Symbols Nerd Font Mono"
            "Noto Serif CJK SC"
            "Noto Serif CJK JP"
            "Noto Serif CJK KR"
          ];
          monospace = [
            "JetBrainsMono Nerd Font"
            "Symbols Nerd Font Mono"
            "Noto Sans Mono CJK SC"
            "Noto Sans Mono CJK KR"
          ];
          emoji = [
            "Symbols Nerd Font Mono"
            "Noto Color Emoji"
          ];
        };
      };
    };

    # The release whose stateful defaults this system was built against. Leave
    # it at the first install's version; see man configuration.nix.
    system.stateVersion = "26.05"; # Did you read the comment?
  };
}
