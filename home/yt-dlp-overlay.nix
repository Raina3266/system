# yt-dlp 2026.07.04 — the version nixpkgs pins — defaults to the YouTube
# clients `android_vr` and `web_safari`. As of August 2026 neither can produce
# a downloadable audio stream:
#
#   android_vr   YouTube 403s every format from this client since 2026-08-17.
#                Extraction succeeds, so you get a format list and then the
#                media fetch dies with `HTTP Error 403: Forbidden` — which
#                spotdl reports as `AudioProviderError: YT-DLP download error`.
#   web_safari   Its HTTPS formats now come back without URLs (YouTube forces
#                SABR streaming for this client, yt-dlp#12482) and what remains
#                needs a GVS PO Token bound to the video ID. yt-dlp can only
#                get one from a PO Token provider plugin, which nixpkgs does
#                not package, so those formats are skipped too.
#
# Upstream fixed this in yt-dlp 4d8e66d (PR yt-dlp/yt-dlp#17461, issue #17456)
# by dropping android_vr in favour of `visionos`: a client that needs no PO
# token, no auth, and no JS player. That commit alone does not port back —
# `visionos` does not exist in 2026.07.04 — so the patch here backports the
# client definition verbatim from master alongside the defaults change.
#
# `tv` is deliberately not used, despite also being PO-token-free: YouTube is
# running an experiment that marks all of its formats DRM-protected
# (yt-dlp#12563), and it returns UNPLAYABLE outright on some sessions.
#
# If visionos ever stops working too, the fallback that needs no PO token is
# authentication: logged-in sessions default to `tv_downgraded`, so passing
# cookies (`spotdl --cookie-file`, `yt-dlp --cookies-from-browser`) routes
# around all of this.
#
# This is an overlay rather than a `home.packages` override because spotdl
# imports yt-dlp as a *library* (`python3Packages.yt-dlp`, a different
# derivation from the `yt-dlp` CLI). nixpkgs defines the Python module as
# `toPythonModule (pkgs.yt-dlp.override { ... })`, so patching the top-level
# package fixes the CLI and spotdl's copy in one go.
#
# Delete this once nixpkgs ships a yt-dlp newer than 2026.07.04 — the patch
# will fail to apply rather than silently do nothing.
final: prev: {
  yt-dlp = prev.yt-dlp.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ ./yt-dlp-visionos.patch ];
  });
}
