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

    elephant = {
      url = "github:abenz1267/elephant";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    walker = {
      url = "github:abenz1267/walker";
      inputs.elephant.follows = "elephant";
      inputs.nixpkgs.follows = "nixpkgs";
    };
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
            walker = inputs.walker.packages.${system}.default;
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
