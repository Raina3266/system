#!/nix/store/xxxx-bash/bin/bash
# Patches applied to Waybar's niri/workspaces module at build time.
# These modify src/modules/niri/workspace.cpp and
# include/modules/niri/workspaces.hpp before compilation.
#
# Patches:
#   1. Make getRewrite() public so Workspace can call it for text labels.
#   2. Fix hide-empty to work together with current-only.
#   3. Hide the taskbar box when there are no windows (no label re-show).
#      Box left-aligned, no hexpand — natural size when few tabs.
#   4. Buttons shrink when overflowing.
#   5. Render icon + text title (20 chars) in each taskbar button.

set -e

# 1. Make getRewrite public so Workspace can call it for text labels.
sed -i 's|  std::string getRewrite(const std::string\& app_id, const std::string\& title);||' \
  include/modules/niri/workspaces.hpp
sed -i 's|  std::string getIcon(const std::string\& value, const Json::Value\& ws) const;|&\n  std::string getRewrite(const std::string\& app_id, const std::string\& title);|' \
  include/modules/niri/workspaces.hpp

# 2. Fix visibility: make hide-empty work together with current-only.
# Upstream uses if/else-if so current-only takes precedence and
# hide-empty is never checked. Replace the single-line ternary with
# a block that also checks hide-empty + active_window_id.
perl -0777 -i -pe 's/    data\[prop\]\.asBool\(\) \? button_\.show\(\) : button_\.hide\(\);/    bool should_show = data[prop].asBool();\n    if (should_show \&\& cfg["hide-empty"].asBool() \&\& data["active_window_id"].isNull() \&\& !data["is_focused"].asBool()) {\n      should_show = false;\n    }\n    should_show ? button_.show() : button_.hide();/' \
  src/modules/niri/workspace.cpp

# 3. Hide the taskbar box when there are no windows on the workspace.
# Do NOT re-show the workspace label — just hide everything.
# Box is left-aligned with hexpand=false so it takes only its natural
# width — buttons stay at natural size when there are few tabs.
perl -0777 -i -pe 's/    rebuildTaskbar\(my_windows\);\n    taskbar_box_\.show\(\);\n    label_\.hide\(\);/    rebuildTaskbar(my_windows);\n    if (my_windows.empty()) {\n      taskbar_box_.hide();\n      label_.hide();\n    } else {\n      taskbar_box_.set_hexpand(true);\n      taskbar_box_.set_halign(Gtk::ALIGN_START);\n      taskbar_box_.show();\n      label_.hide();\n    }/' \
  src/modules/niri/workspace.cpp

# 4. Render icon + text title in each taskbar button.
# Upstream shows either an icon OR a 3-char fallback. Replace with:
# an icon (if available) followed by just the window title (truncated
# to 20 chars). No app_id prefix — just the title. If the title is
# empty, fall back to the rewritten app name.
# Keep each button's minimum width equal to its displayed title length,
# capped at 20 characters by the truncation below.
REPLACEMENT='    auto* btn_box = Gtk::make_managed<Gtk::Box>(Gtk::ORIENTATION_HORIZONTAL, 3);
    btn_box->set_halign(Gtk::ALIGN_START);
    auto pixbuf = loadIcon(app_id, icon_size);
    if (pixbuf) {
      auto* img = Gtk::make_managed<Gtk::Image>(pixbuf);
      btn_box->pack_start(*img, false, false, 0);
    }
    std::string label_text = title;
    if (label_text.empty() || label_text == app_id) {
      label_text = manager_.getRewrite(app_id, title);
      if (label_text.empty() || label_text == "?" || label_text == app_id) {
        label_text = "?";
      }
    }
    const std::string separator = " - ";
    const auto first_separator = label_text.find(separator);
    const auto last_separator = label_text.rfind(separator);
    if (first_separator != std::string::npos &&
        last_separator != first_separator &&
        last_separator > first_separator + separator.length()) {
      const std::string outer_prefix = label_text.substr(0, first_separator);
      const std::string outer_suffix = label_text.substr(last_separator + separator.length());
      if (!outer_prefix.empty() && outer_prefix == outer_suffix) {
        label_text = label_text.substr(
            first_separator + separator.length(),
            last_separator - first_separator - separator.length());
      }
    }
    if (label_text.length() > 20) {
      label_text = label_text.substr(0, 20);
    }
    auto* lbl = Gtk::make_managed<Gtk::Label>(label_text);
    lbl->set_ellipsize(Pango::ELLIPSIZE_END);
    lbl->set_xalign(0.0);
    lbl->set_single_line_mode(true);
    int min_chars = (int)label_text.length();
    if (min_chars > 20) min_chars = 20;
    lbl->set_width_chars(min_chars);
    btn_box->pack_start(*lbl, true, true, 0);
    btn->add(*btn_box);'

perl -0777 -i -pe "s/    auto pixbuf = loadIcon\\(app_id, icon_size\\);\\n    if \\(pixbuf\\) \\{\\n      auto\\* img = Gtk::make_managed<Gtk::Image>\\(pixbuf\\);\\n      btn->add\\(\\*img\\);\\n    \\} else \\{\\n      std::string fallback = app_id.empty\\(\\) \\? title : app_id;\\n      if \\(!fallback.empty\\(\\)\\) \\{\\n        fallback = fallback.substr\\(0, 3\\);\\n      \\} else \\{\\n        fallback = \"\\?\";\\n      \\}\\n      auto\\* lbl = Gtk::make_managed<Gtk::Label>\\(fallback\\);\\n      btn->add\\(\\*lbl\\);\\n    \\}/$REPLACEMENT/" \
  src/modules/niri/workspace.cpp
