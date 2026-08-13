{ inputs, pkgs, ... }:
{
  home.packages = with pkgs; [
    openssl
    gcc
    nil
    nixd
    uv
    rustup
    mprisence
    cargo-machete
    diesel-cli
    cargo-audit
    elan
    cargo-autoinherit
  ];

  targets.genericLinux.nixGL.packages = inputs.nixGL.packages;
  targets.genericLinux.nixGL.defaultWrapper = "mesa";
  targets.genericLinux.nixGL.installScripts = [ "mesa" ];
}
