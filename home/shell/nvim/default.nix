{ inputs, ... }:
{
  imports = [ inputs.nixvim.homeModules.nixvim ];
  config = {
    programs.nixvim.enable = true;
  };
}
