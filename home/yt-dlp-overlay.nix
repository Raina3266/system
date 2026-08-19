# yt-dlp 2026.07.04 picks `android_vr` as one of its two default YouTube
# clients. On 2026-08-17 YouTube started rejecting *every* android_vr format
# with HTTP 403, so extraction still succeeds — you get a format list — and
# then the media download dies:
#
#   ERROR: unable to download video data: HTTP Error 403: Forbidden
#
# via spotdl that surfaces as the generic `AudioProviderError: YT-DLP download
# error - <url>`, which is why it looks like a spotdl bug and isn't.
#
# Upstream fixed it the day after in yt-dlp 4d8e66d ("[ie/youtube] Remove
# `android_vr` from default clients", PR yt-dlp/yt-dlp#17461, issue #17456) by
# dropping android_vr from the defaults. That commit does not apply to
# 2026.07.04 — master's list is ('visionos', 'android_vr', 'web') and the
# `visionos` client does not exist in this release — so this reproduces the
# same change against the pinned version instead of backporting the diff.
#
# `tv` takes the vacated slot rather than leaving `web_safari` alone: the
# web clients require a GVS PO token for their HTTPS/DASH formats, and yt-dlp
# skips those when it has no PO token provider, leaving only web_safari's
# pre-merged HLS streams. That means no audio-only format for spotdl to pick,
# so it would fall back to pulling whole videos. `tv` needs no PO token and
# still offers audio-only formats. (It does need a JS runtime for the nsig
# challenge — nixpkgs already bakes Deno into this derivation.)
#
# `_DEFAULT_JSLESS_CLIENTS` is deliberately left on android_vr: it is only
# consulted when no JS runtime is available at all, which is not the case
# here, and this release has no PO-token-free client that also works without
# JS to replace it with.
#
# This is an overlay rather than a `home.packages` override because spotdl
# imports yt-dlp as a *library* (`python3Packages.yt-dlp`, a different
# derivation from the `yt-dlp` CLI). nixpkgs defines the Python module as
# `toPythonModule (pkgs.yt-dlp.override { ... })`, so overriding the top-level
# package fixes the CLI and spotdl's copy in one go.
#
# Delete this file once nixpkgs ships a yt-dlp newer than 2026.07.04 — the
# --replace-fail will break the build rather than silently patch nothing.
final: prev: {
  yt-dlp = prev.yt-dlp.overrideAttrs (old: {
    postPatch = (old.postPatch or "") + ''
      substituteInPlace yt_dlp/extractor/youtube/_video.py \
        --replace-fail "_DEFAULT_CLIENTS = ('android_vr', 'web_safari')" "_DEFAULT_CLIENTS = ('tv', 'web_safari')"
    '';
  });
}
