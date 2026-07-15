{
  nixConfig = {
    extra-substituters = [
      "https://zed.cachix.org"
      "https://cache.garnix.io"
    ];
    extra-trusted-public-keys = [
      "zed.cachix.org-1:/pHQ6dpMsAZk2DiP4WCL0p9YDNKWj2Q5FL20bNmw1cU="
      "cache.garnix.io:CTFPyKSLcx5RMJKfLo5EEPUObbA78b0YQ2DTCJXqr9g="
    ];
  };

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    nixGL.url = "github:nix-community/nixGL";

    waybar = {
      # Tracks Waybar master for the niri/workspaces `workspace-taskbar` mode
      # (PR #4997, merged 2026-07-03, not yet in any release). Re-evaluate
      # whether this overlay is still needed each time nixpkgs bumps waybar.
      url = "github:Alexays/Waybar/d4a44172106e26ddc5e95e007202113d3141d03a";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    home-manager.url = "github:nix-community/home-manager";
    home-manager.inputs.nixpkgs.follows = "nixpkgs";

    nixvim = {
      url = "github:nix-community/nixvim";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    zed.url = "github:zed-industries/zed/nightly";

    elephant = {
      url = "github:abenz1267/elephant";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    walker = {
      url = "github:abenz1267/walker";
      inputs.elephant.follows = "elephant";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      home-manager,
      nixGL,
      nixvim,
      ...
    }@inputs:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [
          nixGL.overlay
          (final: prev: {
            # Waybar from master (for niri/workspaces workspace-taskbar mode,
            # PR #4997) with a local patch adding a `text-only` option to
            # workspace-taskbar so window buttons show text labels instead
            # of icons. Re-evaluate whether this overlay is still needed
            # each time nixpkgs bumps waybar.
            waybar = inputs.waybar.packages.${system}.default.overrideAttrs (old: {
              nativeBuildInputs = (old.nativeBuildInputs or [ ]) ++ [ final.perl ];
              postPatch = (old.postPatch or "") + ''
                # Make getRewrite public so Workspace can call it for text labels.
                sed -i 's|  std::string getRewrite(const std::string\& app_id, const std::string\& title);||' \
                  include/modules/niri/workspaces.hpp
                sed -i 's|  std::string getIcon(const std::string\& value, const Json::Value\& ws) const;|&\n  std::string getRewrite(const std::string\& app_id, const std::string\& title);|' \
                  include/modules/niri/workspaces.hpp

                # Add text-only config option reading after icon_size.
                sed -i 's|const int icon_size = taskbar_cfg\["icon-size"\].isInt() ? taskbar_cfg\["icon-size"\].asInt() : 16;|&\n    const bool text_only = taskbar_cfg["text-only"].isBool() ? taskbar_cfg["text-only"].asBool() : false;|' \
                  src/modules/niri/workspace.cpp

                # Hide the taskbar box when there are no windows on the
                # workspace, so the entire taskbar module disappears on
                # empty workspaces. Also re-show the workspace label so the
                # button isn't completely blank.
                perl -0777 -i -pe 's/    rebuildTaskbar\(my_windows\);\n    taskbar_box_\.show\(\);\n    label_\.hide\(\);/    rebuildTaskbar(my_windows);\n    if (my_windows.empty()) {\n      taskbar_box_.hide();\n      label_.show();\n    } else {\n      taskbar_box_.show();\n      label_.hide();\n    }/' \
                  src/modules/niri/workspace.cpp

                # Replace the icon-or-fallback block with text-only logic.
                # Use a perl one-liner for multi-line replacement.
                # The label shows: rewritten app name + ": " + truncated title
                # (first 20 chars) so multiple windows of the same app are
                # distinguishable. Total label capped at 35 chars.
                perl -0777 -i -pe 's/    auto pixbuf = loadIcon\(app_id, icon_size\);\n    if \(pixbuf\) \{\n      auto\* img = Gtk::make_managed<Gtk::Image>\(pixbuf\);\n      btn->add\(\*img\);\n    \} else \{\n      std::string fallback = app_id.empty\(\) \? title : app_id;\n      if \(!fallback.empty\(\)\) \{\n        fallback = fallback.substr\(0, 3\);\n      \} else \{\n        fallback = "\?";\n      \}\n      auto\* lbl = Gtk::make_managed<Gtk::Label>\(fallback\);\n      btn->add\(\*lbl\);\n    \}/    if (!text_only) {\n      auto pixbuf = loadIcon(app_id, icon_size);\n      if (pixbuf) {\n        auto* img = Gtk::make_managed<Gtk::Image>(pixbuf);\n        btn->add(*img);\n        taskbar_box_.pack_start(*btn, false, false, 0);\n        btn->show_all();\n        continue;\n      }\n    }\n    std::string app_label = manager_.getRewrite(app_id, title);\n    if (app_label.empty() || app_label == "?") {\n      app_label = app_id.empty() ? title : app_id;\n      if (app_label.empty()) app_label = "?";\n    }\n    std::string label_text = app_label;\n    if (!title.empty() && title != app_id) {\n      std::string title_short = title.length() > 20 ? title.substr(0, 20) : title;\n      label_text = app_label + ": " + title_short;\n    }\n    if (label_text.length() > 35) {\n      label_text = label_text.substr(0, 35);\n    }\n    auto* lbl = Gtk::make_managed<Gtk::Label>(label_text);\n    btn->add(*lbl);/' \
                  src/modules/niri/workspace.cpp
              '';
            });
            walker = inputs.walker.packages.${system}.default;
          })
        ];
        config = {
          allowUnfree = true;
          packageOverrides = pkgs: {
            intel-vaapi-driver = pkgs.intel-vaapi-driver.override {
              enableHybridCodec = true;
            };
          };
        };
      };
    in
    {
      # sudo nixos-rebuild switch --flake .#raina
      nixosConfigurations.raina = nixpkgs.lib.nixosSystem {
        inherit system;
        inherit pkgs;
        specialArgs = {
          inputs = inputs;
        };
        modules = [
          ./nixos/configuration.nix
          ./nixos/hardware.nix
          ./nixos/webcam-crop.nix
        ];
      };
    };
}
