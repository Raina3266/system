# Builds the Nightingale Tauri app from its upstream .deb release.
#
# This is a standard nixpkgs-style package recipe: a function whose arguments
# are supplied by `callPackage` from the overlay in ../default.nix. The .deb is
# fetched, unpacked with dpkg-deb, and its prebuilt ELF binaries are patched by
# autoPatchelfHook so they find their shared libraries under Nix.
{
  stdenv,
  fetchurl,
  dpkg,
  autoPatchelfHook,
  makeWrapper,
  gtk3,
  webkitgtk_4_1,
  glib,
  glibc,
  cairo,
  pango,
  atk,
  gdk-pixbuf,
  libsoup_3,
  openssl,
  alsa-lib,
  zlib,
  libGL,
}:

let
  pname = "nightingale";
  version = "0.8.0";
in
stdenv.mkDerivation {
  inherit pname version;

  src = fetchurl {
    url = "https://github.com/rzru/nightingale/releases/download/v${version}/nightingale_${version}_amd64.deb";
    hash = "sha256:f2e6d9a6ff5a8e1382813f8847383f0fa0368ae78e8f531966f507e6db920f9f";
  };

  # dpkg-deb extracts the .deb; autoPatchelfHook rewrites the prebuilt ELF
  # binaries' RPATH/interp to point at buildInputs; makeWrapper is available
  # in case a wrapper script is needed later.
  nativeBuildInputs = [
    dpkg
    autoPatchelfHook
    makeWrapper
  ];

  # Libraries the Tauri/WebKit UI and the ML pipeline link against at runtime.
  # Mirrors the dependency set the original FHS wrapper pulled in.
  buildInputs = [
    gtk3
    webkitgtk_4_1
    glib
    glibc
    cairo
    pango
    atk
    gdk-pixbuf
    libsoup_3
    openssl
    alsa-lib
    zlib
    libGL
    stdenv.cc.cc.lib
  ];

  # Force autoPatchelf to run after install, against the laid-out tree.
  dontAutoPatchelf = false;

  unpackPhase = ''
    dpkg-deb -x $src .
  '';

  # Prebuilt binary package: nothing to configure or compile.
  dontConfigure = true;
  dontBuild = true;

  installPhase = ''
    mkdir -p $out
    # .deb layout: usr/{bin,share,...} and optionally opt/<app>/...
    cp -r usr/* $out/ 2>/dev/null || true
    if [ -d opt ]; then
      cp -r opt/* $out/ 2>/dev/null || true
    fi
  '';
}
