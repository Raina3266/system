{ inputs, pkgs, ... }:
{
  home.packages = with pkgs; [
    rustup
    diesel-cli
    cargo-machete
    cargo-audit
    cargo-autoinherit

    elan
    openssl
    gcc
    nil
    nixd
    uv
  ];

  targets.genericLinux.nixGL.packages = inputs.nixGL.packages;
  targets.genericLinux.nixGL.defaultWrapper = "mesa";
  targets.genericLinux.nixGL.installScripts = [ "mesa" ];
}
