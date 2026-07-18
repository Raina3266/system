# Handy speech-to-text configuration.
#
# Uses clipboard paste method (wl-copy) + wtype for typing, which works on
# both niri and GNOME Wayland (both support virtual-keyboard-v1 + wl-copy).
#
# Handy stores settings in two places:
#   1. settings_store.json — the "source of truth" on disk
#   2. Tauri WebView local storage — a cached copy that the UI reads/writes
#
# Also,
# handy (>= 0.9) rewrites settings_store.json with factory defaults
# whenever the file's settings_schema_version doesn't match the running
# binary's compiled-in schema version — which happens after every handy
# package upgrade, clobbering the config below.
#
# The robust fix is to make the systemd user service self-healing: every
# time Handy starts (boot, `nixos-rebuild switch`, crash restart, manual
# `systemctl --user restart handy`), the ExecStartPre script below
#   - kills any stale handy instance (so it can't flush stale state back
#     over the file we're about to write),
#   - writes the Nix-managed settings_store.json (bumping the schema
#     version to whatever the current package expects),
#   - wipes the WebView cache (so handy can't resurrect old settings
#     from local storage).
# After that, handy always comes up with exactly the config in this file.
#
# Earlier iterations applied the settings from a home-manager activation
# script instead.  That had two unfixable races:
#   1. `pkill -x handy` never matched the running process — Nix wraps GUI
#      binaries, so the process name is `.handy-wrapped`.  The "stop it
#      first" step was a silent no-op (swallowed by `|| true`), so every
#      activation wiped the WebView cache out from under a still-running
#      handy, which then flushed its corrupted in-memory state back to
#      settings_store.json.
#   2. At boot, activation runs before the user systemd instance exists,
#      so its systemctl stop/start were also silent no-ops — leaving
#      whatever state the last shutdown had, including a defaults-clobbered
#      file after a handy upgrade.
#
# The downloaded model lives in the HuggingFace cache
# (~/.cache/huggingface/hub), which persists across reboots on the /home
# btrfs subvolume, so it only needs to be downloaded once.
{
  pkgs,
  config,
  lib,
  ...
}:
let
  # Schema version the currently packaged handy writes.  If an activation
  # ever resets settings to blank defaults again, bump this to whatever
  # `jq .settings.settings_schema_version \
  #   ~/.local/share/com.pais.handy/settings_store.json` reports.
  handySettings = builtins.toJSON {
    settings = {
      always_on_microphone = false;
      app_language = "en-GB";
      append_trailing_space = false;
      audio_feedback = false;
      audio_feedback_volume = 1.0;
      auto_submit = false;
      auto_submit_key = "enter";
      autostart_enabled = true;
      bindings = {
        cancel = {
          current_binding = "escape";
          default_binding = "escape";
          description = "Cancels the current recording.";
          id = "cancel";
          name = "Cancel";
        };
        transcribe = {
          current_binding = "ctrl+space";
          default_binding = "ctrl+space";
          description = "Converts your speech into text.";
          id = "transcribe";
          name = "Transcribe";
        };
        transcribe_with_post_process = {
          current_binding = "ctrl+shift+space";
          default_binding = "ctrl+shift+space";
          description = "Converts your speech into text and applies AI post-processing.";
          id = "transcribe_with_post_process";
          name = "Transcribe with Post-Processing";
        };
      };
      clamshell_microphone = null;
      clipboard_handling = "dont_modify";
      custom_filler_words = null;
      custom_words = [ ];
      debug_mode = false;
      experimental_enabled = false;
      external_script_path = null;
      extra_recording_buffer_ms = 0;
      history_limit = 5;
      keyboard_implementation = "tauri";
      lazy_stream_close = false;
      log_level = "debug";
      model_unload_timeout = "min5";
      mute_while_recording = false;
      onboarding_completed = false;
      ort_accelerator = "auto";
      overlay_position = "bottom";
      overlay_style = "minimal";
      paste_delay_ms = 60;
      paste_method = "direct";
      post_process_api_keys = {
        anthropic = "";
        bedrock_mantle = "";
        cerebras = "";
        custom = "";
        groq = "";
        openai = "";
        openrouter = "";
        zai = "";
      };
      post_process_enabled = false;
      post_process_models = {
        anthropic = "";
        bedrock_mantle = "";
        cerebras = "";
        custom = "";
        groq = "";
        openai = "";
        openrouter = "";
        zai = "";
      };
      post_process_prompts = [
        {
          id = "default_improve_transcriptions";
          name = "Improve Transcriptions";
          prompt = "Clean this transcript:\n1. Fix spelling, capitalization, and punctuation errors\n2. Convert number words to digits (twenty-five → 25, ten percent → 10%, five dollars → $5)\n3. Replace spoken punctuation with symbols (period → ., comma → ,, question mark → ?)\n4. Remove filler words (um, uh, like as filler)\n5. Keep the language in the original version (if it was french, keep it in french for example)\n\nPreserve exact meaning and word order. Do not paraphrase or reorder content.\n\nReturn only the cleaned transcript.\n\nTranscript:\n\${output}";
        }
      ];
      post_process_provider_id = "openai";
      post_process_providers = [
        {
          allow_base_url_edit = false;
          base_url = "https://api.openai.com/v1";
          id = "openai";
          label = "OpenAI";
          models_endpoint = "/models";
          supports_structured_output = true;
        }
        {
          allow_base_url_edit = false;
          base_url = "https://api.z.ai/api/paas/v4";
          id = "zai";
          label = "Z.AI";
          models_endpoint = "/models";
          supports_structured_output = true;
        }
        {
          allow_base_url_edit = false;
          base_url = "https://openrouter.ai/api/v1";
          id = "openrouter";
          label = "OpenRouter";
          models_endpoint = "/models";
          supports_structured_output = true;
        }
        {
          allow_base_url_edit = false;
          base_url = "https://api.anthropic.com/v1";
          id = "anthropic";
          label = "Anthropic";
          models_endpoint = "/models";
          supports_structured_output = false;
        }
        {
          allow_base_url_edit = false;
          base_url = "https://api.groq.com/openai/v1";
          id = "groq";
          label = "Groq";
          models_endpoint = "/models";
          supports_structured_output = false;
        }
        {
          allow_base_url_edit = false;
          base_url = "https://api.cerebras.ai/v1";
          id = "cerebras";
          label = "Cerebras";
          models_endpoint = "/models";
          supports_structured_output = true;
        }
        {
          allow_base_url_edit = false;
          base_url = "https://bedrock-mantle.us-east-1.api.aws/v1";
          id = "bedrock_mantle";
          label = "AWS Bedrock (Mantle)";
          models_endpoint = "/models";
          supports_structured_output = true;
        }
        {
          allow_base_url_edit = true;
          base_url = "http://localhost:11434/v1";
          id = "custom";
          label = "Custom";
          models_endpoint = "/models";
          supports_structured_output = false;
        }
      ];
      post_process_selected_prompt_id = null;
      push_to_talk = true;
      recording_retention_period = "preserve_limit";
      selected_language = "auto";
      selected_microphone = null;
      selected_model = "";
      selected_output_device = null;
      settings_schema_version = 1;
      show_tray_icon = true;
      show_whats_new_on_update = true;
      sound_theme = "marimba";
      start_hidden = true;
      transcribe_accelerator = "auto";
      transcribe_gpu_device = -1;
      translate_to_english = false;
      typing_tool = "auto";
      update_checks_enabled = true;
      vad_enabled = true;
      whats_new_last_seen_version = "0.9.0";
      word_correction_threshold = 0.18;
    };
  };

  # Applied before every handy start to ensure consistent settings.
  applySettings = pkgs.writeShellScript "handy-apply-settings" ''
    set -u
    dataDir="$HOME/.local/share/com.pais.handy"

    # Kill any stale handy still running (e.g. an instance started outside
    # systemd, or one whose unit file pointed at an older store path).
    # Match on the full command line: Nix-wrapped binaries don't run under
    # the plain "handy" process name that `pkill -x` requires.
    ${pkgs.procps}/bin/pkill -f '/bin/handy( |$)' 2>/dev/null || true

    mkdir -p "$dataDir"
    cat > "$dataDir/settings_store.json" << 'HANDY_EOF'
    ${handySettings}
    HANDY_EOF
  '';
in
{
  home.packages = with pkgs; [
    handy
    wtype
    wl-clipboard
  ];

  # Handy will be started by niri's spawn-at-startup instead of systemd
  # systemd.user.services.handy = {
  #   Unit = {
  #     Description = "Handy — speech-to-text";
  #     ConditionEnvironment = [ "WAYLAND_DISPLAY" ];
  #     PartOf = [ "graphical-session.target" ];
  #     After = [ "graphical-session.target" ];
  #   };
  #   Service = {
  #     ExecStartPre = "${applySettings}";
  #     ExecStart = "${pkgs.handy}/bin/handy --start-hidden";
  #     Restart = "on-failure";
  #     RestartSec = 3;
  #     TimeoutStopSec = 10;
  #   };
  #   Install = {
  #     WantedBy = [ "graphical-session.target" ];
  #   };
  # };

  # Apply settings when home-manager activates, since handy is now
  # started by niri instead of systemd
  home.activation.handySettings = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    ${applySettings}
  '';
}
