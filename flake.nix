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
  };

  outputs =
    {
      nixpkgs,
      ...
    }@inputs:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in
    {
      nixosConfigurations.raina = nixpkgs.lib.nixosSystem {
        inherit system;
        specialArgs = {
          inherit inputs;
        };
        modules = [
          ./nixos/configuration.nix
          ./nixos/hardware.nix
          ./nixos/webcam-crop.nix
        ];
      };

      # Devshell for the Rust projects under scripts/. Nix builds give each
      # derivation its own pkg-config and system libraries via
      # nativeBuildInputs/buildInputs, but rust-analyzer running in the editor
      # has only the user profile on PATH — so the -sys crates (glib-sys,
      # gtk4-sys, libdbus-sys, …) fail their build scripts and RA can't analyze
      # the workspace. direnv loads this shell via the root .envrc so any edit
      # anywhere in this repo gets the right PKG_CONFIG_PATH.
      devShells.${system}.rust = pkgs.mkShell {
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
