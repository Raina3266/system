{
  nixConfig = {
    extra-substituters = [
      "https://zed.cachix.org"
    ];
    extra-trusted-public-keys = [
      "zed.cachix.org-1:/pHQ6dpMsAZk2DiP4WCL0p9YDNKWj2Q5FL20bNmw1cU="
    ];
  };

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    nixGL.url = "github:nix-community/nixGL";

    home-manager.url = "github:nix-community/home-manager";
    home-manager.inputs.nixpkgs.follows = "nixpkgs";

    nixvim = {
      url = "github:nix-community/nixvim";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    zed.url = "github:zed-industries/zed/nightly";

    # walker/elephant come from nixpkgs and are configured through
    # home-manager's upstream services.walker / services.elephant
    # modules, so no flake inputs are needed for them.
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
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [
          nixGL.overlay
          (final: prev: {
            # rofi = the bash wrapper, rofi-unwrapped = the compiled C program.
            # Pinned to the merge commit of PR #2272, which implements
            # click-to-exit on Wayland (fullscreen capture surface).
            rofi-unwrapped = prev.rofi-unwrapped.overrideAttrs (old: {
              version = "2.0.0-dev";
              src = final.fetchFromGitHub {
                owner = "davatorium";
                repo = "rofi";
                rev = "6d2a5281e45dee92dfbdaf6f9ba6081c4c608682";
                fetchSubmodules = true;
                hash = "sha256-4F76JPNaM43DgnM+F0WoYvL5aBbyPSZt3q0YWKAQ9Zs=";
              };
            });
          })
        ];
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
      # sudo nixos-rebuild switch --flake .#raina
      nixosConfigurations.raina = nixpkgs.lib.nixosSystem {
        inherit system;
        inherit pkgs;
        specialArgs = {
          inputs = inputs;
        };
        modules = [
          ./nixos/configuration.nix
          ./nixos/hardware.nix
          ./nixos/webcam-crop.nix
        ];
      };
    };
}
