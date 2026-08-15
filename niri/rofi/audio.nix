# Rofi audio device picker (script mode): lists WirePlumber sinks/sources, sets
# the default on Return, and exposes volume +/-5%, mute and outputs/inputs
# toggle via rofi hotkeys. Replaces the former Walker "menus:audio" provider.
{ pkgs }:
let
  # writeShellScriptBin bakes wpctl's absolute path onto PATH so the script does
  # not depend on wireplumber being on the consumer's environment. The script
  # body is kept in rofi-audio.sh so it can be edited without a Nix rebuild.
  rofiAudio = pkgs.writeShellScriptBin "rofi-audio" (
    builtins.replaceStrings
      [ "@wpctlPath@" ]
      [ "${pkgs.lib.makeBinPath [ pkgs.wireplumber ]}" ]
      (builtins.readFile ./rofi-audio.sh)
  );
in
{
  inherit rofiAudio;
}