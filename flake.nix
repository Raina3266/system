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

    # Builds the scripts/ workspace's dependencies once into a shared artifact
    # the per-crate derivations reuse. It declares no inputs of its own, so
    # there is no nixpkgs to follow; `crane.mkLib pkgs` binds it to ours.
    crane.url = "github:ipetkov/crane";

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

      # Devshell for scripts/. Nix gives each derivation its own pkg-config and
      # libraries, but a plain shell — and the editor's rust-analyzer — sees
      # only the user profile, so the -sys crates fail their build scripts.
      # Enter it with `nix develop .#rust`, or a local untracked .envrc.
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
