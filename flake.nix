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
          # niri's vendored `libdisplay-info-sys` crate requires
          # libdisplay-info < 0.4.0, but nixpkgs bumped the system package
          # to 0.4.0 (fixed upstream in nixpkgs PR #546004, not yet on
          # nixos-unstable as of this pin). Give niri its own 0.3.0 copy
          # without touching the system-wide libdisplay-info used
          # elsewhere. Safe to drop once nixpkgs is updated past that fix.
          (final: prev: {
            niri = prev.niri.override {
              libdisplay-info = prev.libdisplay-info.overrideAttrs (old: {
                version = "0.3.0";
                src = prev.fetchFromGitLab {
                  domain = "gitlab.freedesktop.org";
                  owner = "emersion";
                  repo = "libdisplay-info";
                  rev = "0.3.0";
                  hash = "sha256-nXf2KGovNKvcchlHlzKBkAOeySMJXgxMpbi5z9gLrdc=";
                };
              });
            };
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
