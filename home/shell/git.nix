{ lib, pkgs, ... }:
let
  # Languages mergiraf has a tree-sitter grammar for; each gets a merge driver
  # entry in ~/.gitattributes.
  mergirafExtensions = [
    "c"
    "cc"
    "cpp"
    "cs"
    "dart"
    "go"
    "h"
    "hpp"
    "htm"
    "html"
    "java"
    "js"
    "json"
    "jsx"
    "py"
    "rs"
    "sbt"
    "scala"
    "toml"
    "ts"
    "xhtml"
    "xml"
    "yaml"
    "yml"
  ];
in
{
  programs = {
    git = {
      enable = true;
      signing.format = "openpgp";
      settings = {
        user.name = "raina";
        user.email = "cgl0326@outlook.com";
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

    gh.enable = true;
    gh-dash.enable = true;

    difftastic = {
      enable = true;
      options = "inline";
    };
  };

  home.packages = with pkgs; [
    mergiraf
  ];

  home.file.".gitattributes".text = lib.concatMapStrings (
    extension: "*.${extension} merge=mergiraf\n"
  ) mergirafExtensions;
}
