# Handy speech-to-text configuration for Wayland.
# Uses wtype + clipboard paste for cross-compositor compatibility.
# Settings are applied via home-manager activation to prevent
# handy from resetting them on package upgrades.
{
  pkgs,
  config,
  lib,
  ...
}:
let
  handySettings = builtins.toJSON {
    settings = {
      # Core functionality
      push_to_talk = true;
      keyboard_implementation = "tauri"; 
      clipboard_handling = "dont_modify";
      paste_method = "direct";
      selected_model = "handy-computer/nemotron-3.5-asr-streaming-0.6b-gguf/nemotron-3.5-asr-streaming-0.6b-Q8_0.gguf";
      
      # Key bindings
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
          current_binding = "mod+alt+space";
          default_binding = "mod+alt+space";
          description = "Converts your speech into text and applies AI post-processing.";
          id = "transcribe_with_post_process";
          name = "Transcribe with Post-Processing";
        };
      };
      
      # UI preferences
      overlay_position = "bottom";
      overlay_style = "minimal";
      start_hidden = true;
      show_tray_icon = true;
      sound_theme = "marimba";
      
      # Audio settings
      vad_enabled = true;
      audio_feedback = false;
      selected_language = "auto";
      
      # Post-processing (disabled by default)
      post_process_enabled = false;
      post_process_provider_id = "openai";
      
      # Schema and setup
      settings_schema_version = 1;
      onboarding_completed = true;
      autostart_enabled = false;
      
      # Default values for unused features
      always_on_microphone = false;
      app_language = "en-GB";
      append_trailing_space = false;
      audio_feedback_volume = 1.0;
      auto_submit = false;
      auto_submit_key = "enter";
      clamshell_microphone = null;
      custom_filler_words = null;
      custom_words = [];
      debug_mode = false;
      experimental_enabled = false;
      external_script_path = null;
      extra_recording_buffer_ms = 0;
      history_limit = 5;
      lazy_stream_close = false;
      log_level = "debug";
      model_unload_timeout = "min5";
      mute_while_recording = false;
      ort_accelerator = "auto";
      paste_delay_ms = 30;
      post_process_api_keys = {
        anthropic = ""; bedrock_mantle = ""; cerebras = ""; custom = "";
        groq = ""; openai = ""; openrouter = ""; zai = "";
      };
      post_process_models = {
        anthropic = ""; bedrock_mantle = ""; cerebras = ""; custom = "";
        groq = ""; openai = ""; openrouter = ""; zai = "";
      };
      post_process_prompts = [{
        id = "default_improve_transcriptions";
        name = "Improve Transcriptions";
        prompt = "Clean this transcript:\n1. Fix spelling, capitalization, and punctuation errors\n2. Convert number words to digits (twenty-five → 25, ten percent → 10%, five dollars → $5)\n3. Replace spoken punctuation with symbols (period → ., comma → ,, question mark → ?)\n4. Remove filler words (um, uh, like as filler)\n5. Keep the language in the original version (if it was french, keep it in french for example)\n\nPreserve exact meaning and word order. Do not paraphrase or reorder content.\n\nReturn only the cleaned transcript.\n\nTranscript:\n\${output}";
      }];
      post_process_providers = [
        { allow_base_url_edit = false; base_url = "https://api.openai.com/v1"; id = "openai"; label = "OpenAI"; models_endpoint = "/models"; supports_structured_output = true; }
        { allow_base_url_edit = false; base_url = "https://api.z.ai/api/paas/v4"; id = "zai"; label = "Z.AI"; models_endpoint = "/models"; supports_structured_output = true; }
        { allow_base_url_edit = false; base_url = "https://openrouter.ai/api/v1"; id = "openrouter"; label = "OpenRouter"; models_endpoint = "/models"; supports_structured_output = true; }
        { allow_base_url_edit = false; base_url = "https://api.anthropic.com/v1"; id = "anthropic"; label = "Anthropic"; models_endpoint = "/models"; supports_structured_output = false; }
        { allow_base_url_edit = false; base_url = "https://api.groq.com/openai/v1"; id = "groq"; label = "Groq"; models_endpoint = "/models"; supports_structured_output = false; }
        { allow_base_url_edit = false; base_url = "https://api.cerebras.ai/v1"; id = "cerebras"; label = "Cerebras"; models_endpoint = "/models"; supports_structured_output = true; }
        { allow_base_url_edit = false; base_url = "https://bedrock-mantle.us-east-1.api.aws/v1"; id = "bedrock_mantle"; label = "AWS Bedrock (Mantle)"; models_endpoint = "/models"; supports_structured_output = true; }
        { allow_base_url_edit = true; base_url = "http://localhost:11434/v1"; id = "custom"; label = "Custom"; models_endpoint = "/models"; supports_structured_output = false; }
      ];
      post_process_selected_prompt_id = null;
      recording_retention_period = "preserve_limit";
      selected_microphone = null;
      selected_output_device = null;
      show_whats_new_on_update = true;
      transcribe_accelerator = "auto";
      transcribe_gpu_device = -1;
      translate_to_english = false;
      typing_tool = "wtype";
      update_checks_enabled = true;
      whats_new_last_seen_version = "0.9.0";
      word_correction_threshold = 0.18;
    };
  };

  applySettings = pkgs.writeShellScript "handy-apply-settings" ''
    dataDir="$HOME/.local/share/com.pais.handy"
    ${pkgs.procps}/bin/pkill -f '/bin/handy( |$)' 2>/dev/null || true
    mkdir -p "$dataDir"
    cat > "$dataDir/settings_store.json" << 'EOF'
${handySettings}
EOF
  '';
in
{
  home.packages = with pkgs; [ handy wtype wl-clipboard ];

  # Apply settings on home-manager activation (started by niri, not systemd)
  home.activation.handySettings = lib.hm.dag.entryAfter [ "writeBoundary" ] ''
    ${applySettings}
  '';
}
