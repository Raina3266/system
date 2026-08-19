{
  pkgs,
  ...
}:
let
  # spotdl 4.5.2 has two problems that bite on every album download.
  #
  # 1. `KeyError: 'label'`
  #
  #    spotdl/types/album.py reads `album_metadata["label"]` directly. The
  #    Spotify Web API used to always return `label` on a full album object,
  #    but it now omits the key entirely for some albums (most visibly with
  #    `--auth-token`/`--user-auth`, which hits the official API rather than
  #    the scraped fallback). A missing key aborts the whole download with
  #    a traceback instead of just leaving the publisher tag empty.
  #
  #    Upstream tracks this as spotDL/spotify-downloader#2767 and it is still
  #    open — master is 4.5.2, the same version nixpkgs ships, so there is no
  #    release to upgrade to. The fix is the one-liner from the issue: read the
  #    optional fields with `.get()`. song.py already does exactly that for
  #    `label`, so this only brings album.py in line with the rest of the
  #    codebase. `copyrights` (album.py, song.py) and `genres` (artist.py) are
  #    the same class of bug and fail the same way on other albums/artists, so
  #    they are patched here too.
  #
  # 2. `AudioProviderError: YT-DLP download error`
  #
  #    This one is not spotdl's fault at all: yt-dlp's `android_vr` YouTube
  #    client started returning HTTP 403 for every format, and spotdl reports
  #    any yt-dlp exception under this one opaque message (the real error is
  #    only logged at debug level, hence `--log-level DEBUG` to see it). The
  #    actual fix lives in ./yt-dlp-overlay.nix.
  #
  #    What is left here is the Deno wrapper. spotdl checks for a Deno binary
  #    on PATH and nags about `spotdl --download-deno` whenever a download
  #    fails — advice that cannot work on NixOS, since that command fetches a
  #    foreign dynamically linked binary into ~/.spotdl. Putting nixpkgs' Deno
  #    on spotdl's PATH silences the red herring. yt-dlp's own JS runtime is
  #    already handled: nixpkgs hardcodes a Deno store path into the library.
  #
  # The substitutions use --replace-fail, so if a future spotdl bump upstreams
  # these fixes the build fails loudly and this file can be deleted rather than
  # silently patching nothing.
  spotdl' = pkgs.spotdl.overrideAttrs (old: {
    postPatch = (old.postPatch or "") + ''
      substituteInPlace spotdl/types/album.py \
        --replace-fail 'publisher=album_metadata["label"],' 'publisher=album_metadata.get("label", ""),' \
        --replace-fail 'album_metadata["copyrights"]' 'album_metadata.get("copyrights")'

      substituteInPlace spotdl/types/song.py \
        --replace-fail 'raw_album_meta["copyrights"]' 'raw_album_meta.get("copyrights")'

      substituteInPlace spotdl/types/artist.py \
        --replace-fail '"genres": raw_artist_meta["genres"],' '"genres": raw_artist_meta.get("genres", []),'
    '';

    makeWrapperArgs = (old.makeWrapperArgs or [ ]) ++ [
      "--prefix"
      "PATH"
      ":"
      (pkgs.lib.makeBinPath [ pkgs.deno ])
    ];
  });
in
{
  home.packages = [
    spotdl'
  ];
}
