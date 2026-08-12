{
  lib,
  rustPlatform,
  makeWrapper,
  playerctl,
  rofi,
  glib,
}:

rustPlatform.buildRustPackage {
  pname = "media-control";
  version = "0.1.0";

  src = lib.cleanSource ./.;
  cargoLock.lockFile = ./Cargo.lock;

  nativeBuildInputs = [ makeWrapper ];

  postInstall = ''
    install -Dm644 ${../../home/niri/rofi/theme/media-control.rasi} \
      "$out/share/rofi/themes/media-control.rasi"

    wrapProgram "$out/bin/media-control" \
      --set MEDIA_CONTROL_PLAYERCTL "${lib.getExe playerctl}" \
      --set MEDIA_CONTROL_ROFI "${lib.getExe rofi}" \
      --set MEDIA_CONTROL_GDBUS "${lib.getExe' glib "gdbus"}" \
      --set MEDIA_CONTROL_FALLBACK_THEME "$out/share/rofi/themes/media-control.rasi"
  '';

  meta = {
    description = "Dynamic MPRIS media controller for Rofi and Waybar";
    license = lib.licenses.mit;
    mainProgram = "media-control";
    platforms = lib.platforms.linux;
  };
}
