# Keep Home Manager's global Quickshell package on the same compatibility fixes.
{ ... }:
{
  nixpkgs.overlays = [
    (_final: prev: {
      quickshell = prev.quickshell.overrideAttrs (oldAttrs: {
        patches = (oldAttrs.patches or [ ]) ++ [
          (builtins.path {
            path = ./quickshell-compat.patch;
            name = "quickshell-compat.patch";
            recursive = false;
          })
        ];
      });
    })
  ];
}
