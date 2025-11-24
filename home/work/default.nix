{
  lib,
  pkgs,
  nixosConfig,
  ...
}:
{
  config = lib.mkIf nixosConfig.services'.work.enable {
    home.packages =
      with pkgs;
      [
        slack
      ]
      ++ [
        terraform
        terragrunt
        azure-cli
        nodejs
        pnpm
        docker-compose
        wasm-pack
        yq
        playwright
        playwright-test
        playwright-driver
      ];
    home.sessionVariables = {
      PLAYWRIGHT_BROWSERS_PATH = "${pkgs.playwright-driver.browsers}";
      PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS = "true";
    };
  };

}
