# Builds Baidu Netdisk (百度网盘) from its upstream .deb release.
#
# This is a standard nixpkgs-style package recipe: a function whose arguments
# are supplied by `callPackage` from the overlay in ../default.nix. The .deb is
# fetched, unpacked with dpkg-deb, and its prebuilt ELF binaries are patched by
# autoPatchelfHook so they find their shared libraries under Nix.
#
# Baidu Netdisk is a closed-source Electron app. The .deb ships the app under
# opt/baidunetdisk/ with a launcher script that we wrap to set up the right
# runtime environment.
{
  stdenv,
  lib,
  fetchurl,
  dpkg,
  autoPatchelfHook,
  makeWrapper,
  makeDesktopItem,
  copyDesktopItems,
  # Runtime libraries the Electron app links against.
  nss,
  nspr,
  gtk3,
  libnotify,
  libsecret,
  alsa-lib,
  at-spi2-atk,
  at-spi2-core,
  atk,
  cairo,
  cups,
  dbus,
  expat,
  gdk-pixbuf,
  glib,
  pango,
  udev,
  libdrm,
  mesa,
  libgbm,
  # X11 libraries (using new top-level names; xorg.* is deprecated).
  libX11,
  libxcb,
  libXcomposite,
  libXdamage,
  libXext,
  libXfixes,
  libXrandr,
  libXrender,
  libXtst,
  libXi,
  libXcursor,
  libXinerama,
  libXScrnSaver,
  # C++ bindings for the custom browser engine (libbrowserengine.so).
  # The 2.4 series (gtkmm2) provides libgtkmm-2.4, libgdkmm-2.4, libatkmm-1.6,
  # libgiomm-2.4, libpangomm-1.4, libglibmm-2.4, libcairomm-1.0, libsigc-2.0.
  gtkmm2,
  gtk2,
}:

let
  pname = "baidunetdisk";
  version = "8.5.2";

  # Fetched from https://pan.baidu.com/disk/cmsdata?platform=linux&num=1
  # The API returns the .deb URL in the `url_1` field.
  debUrl = "https://pkg-ant.baidu.com/issue/netdisk/LinuxGuanjia/8.5.2.427/baidunetdisk_8.5.2_amd64.deb";

  runtimeLibs = [
    nss
    nspr
    gtk3
    libnotify
    libsecret
    libXScrnSaver
    alsa-lib
    at-spi2-atk
    at-spi2-core
    atk
    cairo
    cups
    dbus
    expat
    gdk-pixbuf
    glib
    pango
    udev
    libdrm
    mesa
    libgbm
    libX11
    libxcb
    libXcomposite
    libXdamage
    libXext
    libXfixes
    libXrandr
    libXrender
    libXtst
    libXi
    libXcursor
    libXinerama
    libXScrnSaver
    # C++ bindings for libbrowserengine.so (2.4 series)
    gtkmm2
    gtk2
    stdenv.cc.cc.lib
  ];
in
stdenv.mkDerivation {
  inherit pname version;

  src = fetchurl {
    url = debUrl;
    hash = "sha256-R3bPt+3520uL6W2xTTYaZ5X5bHRLNv0chdJop3ITOTY=";
  };

  # dpkg-deb extracts the .deb; autoPatchelfHook rewrites the prebuilt ELF
  # binaries' RPATH/interp to point at buildInputs; makeWrapper lets us set
  # runtime env vars; copyDesktopItems installs the .desktop entry.
  nativeBuildInputs = [
    dpkg
    autoPatchelfHook
    makeWrapper
    copyDesktopItems
  ];

  # Libraries the Electron app and its bundled Chromium link against at runtime.
  buildInputs = runtimeLibs;

  # Force autoPatchelf to run after install, against the laid-out tree.
  dontAutoPatchelf = false;
  # The app bundles a node native module (windows-quiet-hours) that links
  # against libpython3.6m.so.1.0. This module is unused on Linux, so we ignore
  # the missing dependency rather than pulling in Python 3.6.
  autoPatchelfIgnoreMissingDeps = [ "libpython3.6m.so.1.0" ];

  unpackPhase = ''
    dpkg-deb -x $src .
  '';

  # Prebuilt binary package: nothing to configure or compile.
  dontConfigure = true;
  dontBuild = true;

  installPhase = ''
    runHook preInstall

    mkdir -p $out/lib/baidunetdisk $out/bin

    # The .deb ships the app under opt/baidunetdisk/.
    cp -r opt/baidunetdisk/* $out/lib/baidunetdisk/

    # Desktop entry, icons, etc. from usr/share/.
    cp -r usr/share $out/share

    # Wrap the launcher so it finds the right libraries and sets up the
    # sandbox/user-data-dir environment that Electron needs under NixOS.
    makeWrapper $out/lib/baidunetdisk/baidunetdisk $out/bin/baidunetdisk \
      --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath runtimeLibs}" \
      --add-flags "--no-sandbox"

    runHook postInstall
  '';

  desktopItems = [
    (makeDesktopItem {
      name = "baidunetdisk";
      exec = "baidunetdisk %U";
      icon = "baidunetdisk";
      desktopName = "Baidu Netdisk";
      comment = "Baidu cloud storage client";
      categories = [
        "Network"
        "FileTransfer"
      ];
      startupWMClass = "baidunetdisk";
    })
  ];

  meta = {
    description = "Baidu Netdisk (百度网盘) - cloud storage client for Linux";
    homepage = "https://pan.baidu.com";
    license = lib.licenses.unfree;
    mainProgram = "baidunetdisk";
    platforms = [ "x86_64-linux" ];
    sourceProvenance = with lib.sourceTypes; [ binaryNativeCode ];
  };
}
