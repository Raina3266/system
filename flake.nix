{
  nixConfig = {
    extra-substituters = [
      "https://zed.cachix.org"
      "https://cache.garnix.io"
    ];
    extra-trusted-public-keys = [
      "zed.cachix.org-1:/pHQ6dpMsAZk2DiP4WCL0p9YDNKWj2Q5FL20bNmw1cU="
      "cache.garnix.io:CTFPyKSLcx5RMJKfLo5EEPUObbA78b0YQ2DTCJXqr9g="
    ];
  };
  
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";

    home-manager.url = "github:nix-community/home-manager";
    home-manager.inputs.nixpkgs.follows = "nixpkgs";

    # allows programs in nixpkgs that use OpenGL to work on non-nixos systems
    nixGL.url = "github:nix-community/nixGL";

    nixvim.url = "github:nix-community/nixvim";
    nixvim.inputs.nixpkgs.follows = "nixpkgs";
    
    zed.url = "github:zed-industries/zed";
  };

  outputs =
    {
      self,
      nixpkgs,
      home-manager,
      nixGL,
      nixvim,
      ...
    }@inputs:
    let
      pkgs = import nixpkgs {
        system = "x86_64-linux";
        overlays = [ nixGL.overlay ];
        config = {
          allowUnfree = true;
          packageOverrides = pkgs: {
            intel-vaapi-driver = pkgs.intel-vaapi-driver.override {
              enableHybridCodec = true;
            };
          };
        };
      };
    in
    {
      nixosConfigurations.rainawork = nixpkgs.lib.nixosSystem {
        inherit pkgs;
        specialArgs = {
          inputs = inputs;
        };
        modules = [
          ./nixos/configuration.nix
          ./nixos/hardware/dell.nix
          {
            # This value determines the NixOS release from which the default
            # settings for stateful data, like file locations and database versions
            # on your system were taken. It‘s perfectly fine and recommended to leave
            # this value at the release version of the first install of this system.
            # Before changing this value read the documentation for this option
            # (e.g. man configuration.nix or on https://nixos.org/nixos/options.html).
            system.stateVersion = "25.05"; # Did you read the comment?

            home-manager.users.raina.home.stateVersion = "23.05";
            home-manager.users.raina.programs.git.settings.user.email = "raina@kaleidoscope.com";
            # Configure keymap in X11
            services.xserver.xkb = {
              layout = "us";
              variant = "";
            };

            fonts = {
              enableDefaultPackages = true;

              packages = with pkgs; [
                noto-fonts
                noto-fonts-cjk-sans
              ];
            };

            networking.hostName = "rainawork"; # Define your hostname.

            services'.work.enable = true;
          }
        ];
      };

      # sudo nixos-rebuild switch --flake .#thinkpad
      nixosConfigurations.rainapersonal = nixpkgs.lib.nixosSystem {
        inherit pkgs;
        specialArgs = {
          inputs = inputs;
        };
        modules = [
          ./nixos/configuration.nix
          ./nixos/hardware/thinkpad.nix
          {
            # This value determines the NixOS release from which the default
            # settings for stateful data, like file locations and database versions
            # on your system were taken. It‘s perfectly fine and recommended to leave
            # this value at the release version of the first install of this system.
            # Before changing this value read the documentation for this option
            # (e.g. man configuration.nix or on https://nixos.org/nixos/options.html).
            system.stateVersion = "23.11"; # Did you read the comment?

            home-manager.users.raina.home.stateVersion = "23.05";
            home-manager.users.raina.programs.git.settings.user.email = "cgl0326@outlook.com";
            # Configure keymap in X11
            services.xserver.xkb = {
              layout = "gb";
              variant = "";
            };
            i18n.inputMethod = {
              enable = true;
              type = "ibus";
              ibus.engines = with pkgs.ibus-engines; [ libpinyin ];
            };
            networking.hostName = "rainapersonal"; # Define your hostname.

            services'.personal.enable = true;
          }
        ];
      };
    };
}
