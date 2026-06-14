# Cropped virtual webcam.
#
# The built-in "Integrated Camera" has a field of view that is too wide and
# exposes no zoom/crop controls. This module builds a virtual webcam that is
# the *only* camera visible to applications:
#
#   real camera (/dev/cam-raw)  --ffmpeg crop+scale-->  /dev/video<N> ("Cropped Webcam")
#
# The real camera nodes are stripped of their `uaccess` udev tag, so logind no
# longer grants the logged-in user an ACL on them. Since `raina` is not in the
# `video` group, no user application (browsers, Zoom, pipewire, ...) can open
# the real camera anymore — they can only see the cropped loopback device.
#
# On-demand behaviour
# -------------------
# A v4l2loopback device only appears to Chrome (and other picky WebRTC apps)
# while a producer is attached and it is in `exclusive_caps=1` mode. Without a
# producer it advertises OUTPUT-only and disappears from the camera picker.
#
# To keep the cropped camera always *selectable* without keeping the real
# camera running, a small supervisor keeps a cheap placeholder producer (a
# static frame, real camera closed) attached while idle, and switches to the
# real cropped feed only once an application actually opens the device. The
# real camera is therefore only powered on (and only cropped) on demand, with
# a brief delay on first frame.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services'.croppedWebcam;

  outputNode = "/dev/video${toString cfg.videoNr}";
  v4l2loopbackCtl = "${config.boot.kernelPackages.v4l2loopback.bin}/bin/v4l2loopback-ctl";
  v4l2Ctl = "${pkgs.v4l-utils}/bin/v4l2-ctl";
  ffmpeg = "${pkgs.ffmpeg}/bin/ffmpeg";
  # fuser lists the PIDs holding a device open in a single pass (one process),
  # instead of forking readlink for every fd of every process on each poll.
  fuser = "${pkgs.psmisc}/bin/fuser";
  # inotifywait lets us block (0% CPU) until an app opens the loopback, rather
  # than polling while idle.
  inotifywait = "${pkgs.inotify-tools}/bin/inotifywait";

  # Number of poll ticks with no consumer before we drop back to the
  # placeholder (and release the real camera). At least one tick.
  idleTicks = let t = cfg.idleSeconds / cfg.pollSeconds; in if t < 1 then 1 else t;

  # A neutral frame shown while the camera is idle / starting up.
  placeholderImage = pkgs.runCommand "cropped-webcam-placeholder.png" { } ''
    ${ffmpeg} -nostdin -loglevel error \
      -f lavfi -i color=c=0x101418:s=${toString cfg.outputSize}x${toString cfg.outputSize} \
      -frames:v 1 -c:v png -f image2 -update 1 -y "$out"
  '';

  startScript = pkgs.writeShellScript "cropped-webcam-supervisor" ''
    set -u

    NODE="${outputNode}"
    SOURCE="${cfg.source}"
    POLL="${toString cfg.pollSeconds}"
    IDLE_TICKS="${toString idleTicks}"
    WARMUP="${toString cfg.warmupSeconds}"

    child=""   # PID of the currently running ffmpeg producer
    mode=""    # "idle" (placeholder) or "active" (real cropped feed)

    run_placeholder() {
      # 1 fps is plenty: sustain_framerate re-serves the last frame to any
      # consumer, so this just has to keep a producer attached (cheaply) so the
      # device keeps advertising capture caps while idle.
      ${ffmpeg} -nostdin -hide_banner -loglevel error \
        -re -loop 1 -framerate 1 -i "${placeholderImage}" \
        -vf "format=yuv420p" \
        -f v4l2 -pix_fmt yuv420p "$NODE" &
      child=$!
      mode="idle"
    }

    run_active() {
      # -analyzeduration 0 / -fflags nobuffer skip ffmpeg's input probing (the
      # format is already known from the device), which is the bulk of the
      # start-up delay before the first frame reaches the consumer.
      ${ffmpeg} -nostdin -hide_banner -loglevel warning \
        -fflags nobuffer -analyzeduration 0 -probesize 32 \
        -f v4l2 \
          -input_format ${cfg.inputFormat} \
          -video_size ${toString cfg.inputWidth}x${toString cfg.inputHeight} \
          -framerate ${toString cfg.framerate} \
          -i "$SOURCE" \
        -vf "crop=ih:ih,scale=${toString cfg.outputSize}:${toString cfg.outputSize},format=yuv420p" \
        -f v4l2 -pix_fmt yuv420p "$NODE" &
      child=$!
      mode="active"
    }

    stop_child() {
      if [ -n "$child" ] && kill -0 "$child" 2>/dev/null; then
        kill "$child" 2>/dev/null || true
        wait "$child" 2>/dev/null || true
      fi
      child=""
      # v4l2loopback is single-producer in exclusive_caps mode; give the kernel
      # a moment to release the output device before opening the next producer,
      # otherwise the new ffmpeg fails with EBUSY.
      sleep 0.5
    }

    cleanup() { stop_child; exit 0; }
    trap cleanup TERM INT

    # cgroup of this service; all our own ffmpeg producers share it.
    OURCG=$(cat /proc/self/cgroup 2>/dev/null)

    # List external processes (i.e. not part of this service) that hold the
    # loopback open. Excluding our whole cgroup -- not just the current child --
    # means a stray/lingering producer of ours can never be mistaken for a real
    # consumer and pin the camera on.
    consumers() {
      local pid cg
      for pid in $(${fuser} "$NODE" 2>/dev/null); do
        cg=$(cat /proc/"$pid"/cgroup 2>/dev/null)
        [ "$cg" = "$OURCG" ] && continue
        echo "$pid:$(cat /proc/"$pid"/comm 2>/dev/null)"
      done | sort -u
    }

    # Wait for the loopback node to exist before configuring it.
    i=0
    while [ ! -e "$NODE" ] && [ "$i" -lt 120 ]; do
      sleep 0.5
      i=$((i + 1))
    done

    # Pin a fixed capture format so the device is always a valid, openable
    # capture source. Unlock keep_format first so a stale format left by a
    # previous run (e.g. with different dimensions) can never block the change.
    ${v4l2Ctl} -d "$NODE" -c keep_format=0 >/dev/null 2>&1 || true
    ${v4l2loopbackCtl} set-caps "$NODE" "YU12:${toString cfg.outputSize}x${toString cfg.outputSize}@${toString cfg.framerate}/1" || true
    # Re-send the last frame to consumers when the producer stalls, and keep the
    # format pinned, so an in-progress stream survives the placeholder<->live
    # producer switch instead of freezing or dropping.
    ${v4l2Ctl} -d "$NODE" -c sustain_framerate=1 >/dev/null 2>&1 || true
    ${v4l2Ctl} -d "$NODE" -c keep_format=1 >/dev/null 2>&1 || true

    run_placeholder

    while true; do
      # ---- IDLE: placeholder attached, real camera off ----
      # Block on an "open" event instead of polling, so idle costs ~no CPU.
      # The -t timeout is just a safety net to periodically re-check and to
      # resurrect the placeholder if it ever died.
      while [ -z "$(consumers)" ]; do
        if [ -n "$child" ] && ! kill -0 "$child" 2>/dev/null; then
          child=""
          run_placeholder
        fi
        ${inotifywait} -q -q -e open -t 10 "$NODE" >/dev/null 2>&1 || true
      done

      # ---- a real consumer appeared ----
      # Let it finish opening on the placeholder before swapping producers.
      # Swapping mid-open makes the loopback briefly lose its capture
      # capability (exclusive_caps), which makes the app report "camera not
      # found". An already-streaming consumer survives the swap. This also
      # avoids powering the real camera on for brief capability probes.
      sleep "$WARMUP"
      [ -z "$(consumers)" ] && continue

      echo "loopback in use by: $(consumers | tr '\n' ' ') -- starting camera"
      stop_child
      run_active

      # ---- ACTIVE: poll until no consumer remains for idleSeconds ----
      idle=0
      while [ "$idle" -lt "$IDLE_TICKS" ]; do
        sleep "$POLL"
        if [ -n "$child" ] && ! kill -0 "$child" 2>/dev/null; then
          child=""
          run_active
        fi
        if [ -n "$(consumers)" ]; then
          idle=0
        else
          idle=$((idle + 1))
        fi
      done

      echo "idle for ${toString cfg.idleSeconds}s -- releasing the camera"
      stop_child
      run_placeholder
    done
  '';
in
{
  options.services'.croppedWebcam = with lib; {
    enable = mkEnableOption "a cropped virtual webcam that replaces the built-in camera";

    source = mkOption {
      type = types.str;
      default = "/dev/cam-raw";
      description = "Stable device path of the real camera that ffmpeg reads from.";
    };

    videoNr = mkOption {
      type = types.int;
      default = 10;
      description = "v4l2loopback device number; the cropped cam is /dev/video<videoNr>.";
    };

    cardLabel = mkOption {
      type = types.str;
      default = "Cropped Webcam";
      description = "Name applications will see for the virtual camera.";
    };

    inputFormat = mkOption {
      type = types.str;
      default = "mjpeg";
      description = "Pixel/stream format requested from the real camera.";
    };

    inputWidth = mkOption {
      type = types.int;
      default = 1280;
      description = ''
        Capture resolution requested from the real camera. 1280x720 keeps the
        sensor start-up fast and CPU low; the square crop of a 720-tall frame
        needs no upscaling for a 720 output.
      '';
    };
    inputHeight = mkOption {
      type = types.int;
      default = 720;
    };
    framerate = mkOption {
      type = types.int;
      default = 30;
    };

    outputSize = mkOption {
      type = types.int;
      default = 720;
      description = ''
        Side length of the square output. The source frame is centre-cropped to
        a square (width trimmed to equal the height) and then scaled to this
        size, narrowing the horizontal field of view.
      '';
    };

    idleSeconds = mkOption {
      type = types.int;
      default = 3;
      description = ''
        How long the cropped camera may sit unused before the real camera is
        released and the supervisor drops back to the idle placeholder.
      '';
    };

    warmupSeconds = mkOption {
      type = types.int;
      default = 2;
      description = ''
        How long a consumer is allowed to settle on the placeholder before the
        real camera is swapped in. Too short and apps can fail to open with
        "camera not found"; this is the main contributor to start-up delay.
      '';
    };

    pollSeconds = mkOption {
      type = types.int;
      default = 1;
      description = "How often (seconds) to check whether an app is using the camera.";
    };
  };

  config = lib.mkIf cfg.enable {
    # v4l2loopback provides the virtual /dev/video<videoNr> sink. exclusive_caps
    # is required so WebRTC apps (Chrome) accept it once a producer is attached.
    boot.extraModulePackages = [ config.boot.kernelPackages.v4l2loopback ];
    boot.kernelModules = [ "v4l2loopback" ];
    boot.extraModprobeConfig = ''
      options v4l2loopback devices=1 video_nr=${toString cfg.videoNr} card_label="${cfg.cardLabel}" exclusive_caps=1 max_buffers=2
    '';

    services.udev.extraRules = ''
      # Stable path to the real RGB sensor for the cropper service.
      SUBSYSTEM=="video4linux", ATTR{name}=="Integrated Camera: Integrated C", ATTR{index}=="0", SYMLINK+="cam-raw"

      # Hide every real built-in camera node (RGB + IR + metadata) from user
      # apps by dropping the uaccess tag so logind grants no ACL. `raina` is
      # not in the `video` group, so the nodes become unopenable for her.
      SUBSYSTEM=="video4linux", ATTR{name}=="Integrated Camera: Integrated C", TAG-="uaccess"
      SUBSYSTEM=="video4linux", ATTR{name}=="Integrated Camera: Integrated I", TAG-="uaccess"

      # Friendly alias for the cropped virtual camera.
      SUBSYSTEM=="video4linux", ATTR{name}=="${cfg.cardLabel}", SYMLINK+="cam-cropped"
    '';

    systemd.services.cropped-webcam = {
      description = "Cropped virtual webcam supervisor (${cfg.source} -> ${outputNode})";
      wantedBy = [ "multi-user.target" ];
      after = [ "systemd-udev-settle.service" ];

      serviceConfig = {
        ExecStart = startScript;
        Restart = "always";
        RestartSec = 2;

        # Runs as root: detecting which app holds the loopback open requires
        # reading other users' /proc/<pid>/fd entries.
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        NoNewPrivileges = true;
      };
    };

    # Handy for inspecting/tuning the cameras (v4l2-ctl --list-devices, etc.).
    environment.systemPackages = [ pkgs.v4l-utils ];
  };
}
