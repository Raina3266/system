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
  };

  outputs =
    {
      nixpkgs,
      ...
    }@inputs:
    let
      pkgs = nixpkgs.legacyPackages.x86_64-linux;
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

      # Devshell for the Rust projects under scripts/. Nix builds give each
      # derivation its own pkg-config and system libraries via
      # nativeBuildInputs/buildInputs, but rust-analyzer running in the editor
      # has only the user profile on PATH — so the -sys crates (glib-sys, 
      # gtk4-sys, libdbus-sys, …) fail their build scripts and RA can't analyze
      # the workspace. direnv loads this shell via the root .envrc so any edit
      # anywhere in this repo gets the right PKG_CONFIG_PATH.
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
