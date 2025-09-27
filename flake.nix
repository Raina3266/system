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

  outputs =
    {
      self,
      nixpkgs,
      home-manager,
      nixGL,
      nixvim,
    }@inputs:
    let
      pkgs = import nixpkgs {
        system = "x86_64-linux";
        overlays = [ nixGL.overlay ];
        config = {
          allowUnfree = true;
          permittedInsecurePackages = [
            "qtwebengine-5.15.19"
          ];
        };
      };
    in
    {
      homeConfigurations."raina" = home-manager.lib.homeManagerConfiguration {
        inherit pkgs;
        extraSpecialArgs = { inherit inputs; };
        modules = [
          ./home
          {
            home.stateVersion = "24.11";
            programs.git.userEmail = "raina@kaleidoscope.com";
          }
        ];
      };

      nixosConfigurations.thinkpad = nixpkgs.lib.nixosSystem {
        inherit pkgs;
        specialArgs = {
          inputs = inputs;
        };
        modules = [
          ./nixos/configuration.nix
          home-manager.nixosModules.home-manager
          {
            home-manager.users.raina.home.stateVersion = "23.05";
            home-manager.users.raina.programs.git.userEmail = "cgl0326@outlook.com";
            home-manager.users.raina.personal.enable = true;
          }
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
