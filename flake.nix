{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    nixGL.url = "github:nix-community/nixGL";

    home-manager.url = "github:nix-community/home-manager";
    home-manager.inputs.nixpkgs.follows = "nixpkgs";

    nixvim = {
      url = "github:nix-community/nixvim";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    sonora = {
      url = "github:nolight132/sonora";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      nixpkgs,
      ...
    }@inputs:
    let
      pkgs = nixpkgs.legacyPackages.${nixpkgs.lib.systems.flakeSystems.x86_64-linux};
    in
    {
      nixosConfigurations.raina = nixpkgs.lib.nixosSystem {
        modules = [
          {
            nixpkgs.hostPlatform = "x86_64-linux";
          }
          ./nixos
        ];
        specialArgs = {
          inherit inputs;
        };
      };

      devShells.x86_64-linux.rust = pkgs.mkShell {
        nativeBuildInputs = [ pkgs.pkg-config ];
        buildInputs = with pkgs; [
          glib
          gtk4
          gtk4-layer-shell
          graphene
          pango
          cairo
          gdk-pixbuf
          dbus
          libpulseaudio
        ];
      };
    };
}
