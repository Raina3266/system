{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";

    home-manager.url = "github:nix-community/home-manager";
    home-manager.inputs.nixpkgs.follows = "nixpkgs";

    # allows programs in nixpkgs that use OpenGL to work on non-nixos systems
    nixGL.url = "github:nix-community/nixGL";

    nixvim.url = "github:nix-community/nixvim";
    nixvim.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = {
    self,
    nixpkgs,
    home-manager,
    nixGL,
    nixvim,
  } @ inputs: {
    nixosConfigurations.thinkpad = nixpkgs.lib.nixosSystem {
      pkgs = import nixpkgs {
        system = "x86_64-linux";
        overlays = [nixGL.overlay];
        config.allowUnfree = true;
      };
      modules = [
        ./nixos/configuration.nix
        home-manager.nixosModules.home-manager
        {
          home-manager.backupFileExtension = "backup";
          home-manager.useUserPackages = true;
          home-manager.users.raina = import ./home;
          home-manager.useGlobalPkgs = true;
          home-manager.extraSpecialArgs = {
            inherit inputs;
          };
        }
      ];
    };
  };
}
