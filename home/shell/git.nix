{ pkgs, ... }:
{
  programs.git = {
    enable = true;

    settings = {
      init.defaultBranch = "master";

      pull.rebase = true;

      push = {
        default = "current";
        autoSetupRemote = true;
      };

      merge."mergiraf" = {
        name = "mergiraf";
        driver = "mergiraf merge --git %O %A %B -s %S -x %X -y %Y -p %P";
      };
    };
  };
  
  programs.gh.enable = true;
  programs.gh-dash.enable = true;
  
  programs.difftastic = {
    enable = true;
    options = "inline";
  };

  home.packages = with pkgs; [
    mergiraf
  ];

  home.file.".gitattributes".text = ''
    *.java merge=mergiraf
    *.rs merge=mergiraf
    *.go merge=mergiraf
    *.js merge=mergiraf
    *.jsx merge=mergiraf
    *.json merge=mergiraf
    *.yml merge=mergiraf
    *.yaml merge=mergiraf
    *.toml merge=mergiraf
    *.html merge=mergiraf
    *.htm merge=mergiraf
    *.xhtml merge=mergiraf
    *.xml merge=mergiraf
    *.c merge=mergiraf
    *.cc merge=mergiraf
    *.h merge=mergiraf
    *.cpp merge=mergiraf
    *.hpp merge=mergiraf
    *.cs merge=mergiraf
    *.dart merge=mergiraf
    *.scala merge=mergiraf
    *.sbt merge=mergiraf
    *.ts merge=mergiraf
    *.py merge=mergiraf
  '';
}
