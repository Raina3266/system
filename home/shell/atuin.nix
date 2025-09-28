{
  programs.atuin = {
    enable = true;
    enableFishIntegration = true;
    settings = {
      filter_mode_shell_up_key_binding = "session";
      enter_accept = true;
    };

    flags = ["--disable-up-arrow"];
  };
}