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
perl -0777 -i -pe 's/    rebuildTaskbar\(my_windows\);\n    taskbar_box_\.show\(\);\n    label_\.hide\(\);/    rebuildTaskbar(my_windows);\n    if (my_windows.empty()) {\n      taskbar_box_.hide();\n      label_.hide();\n    } else {\n      taskbar_box_.set_hexpand(false);\n      taskbar_box_.set_halign(Gtk::ALIGN_START);\n      taskbar_box_.show();\n      label_.hide();\n    }/' \
  src/modules/niri/workspace.cpp

# 4. Let taskbar buttons shrink when the bar overflows.
# Upstream packs each button with pack_start(*btn, false, false, 0)
# which means buttons don't expand — they overflow off-screen when
# there are too many. Change to expand+fill so buttons can shrink and
# share the available space.
sed -i 's|taskbar_box_.pack_start(\*btn, false, false, 0);|taskbar_box_.pack_start(*btn, true, true, 0);|' \
  src/modules/niri/workspace.cpp

# 5. Render icon + text title in each taskbar button.
# Upstream shows either an icon OR a 3-char fallback. Replace with:
# an icon (if available) followed by just the window title (truncated
# to 20 chars). No app_id prefix — just the title. If the title is
# empty, fall back to the rewritten app name.

perl -0777 -i -pe 's/    auto pixbuf = loadIcon\(app_id, icon_size\);\n    if \(pixbuf\) \{\n      auto\* img = Gtk::make_managed<Gtk::Image>\(pixbuf\);\n      btn->add\(\*img\);\n    \} else \{\n      std::string fallback = app_id.empty\(\) \? title : app_id;\n      if \(!fallback.empty\(\)\) \{\n        fallback = fallback.substr\(0, 3\);\n      \} else \{\n        fallback = "\?";\n      \}\n      auto\* lbl = Gtk::make_managed<Gtk::Label>\(fallback\);\n      btn->add\(\*lbl\);\n    \}/    auto* btn_box = Gtk::make_managed<Gtk::Box>(Gtk::ORIENTATION_HORIZONTAL, 5);\n    btn_box->set_halign(Gtk::ALIGN_CENTER);\n    auto pixbuf = loadIcon(app_id, icon_size);\n    if (pixbuf) {\n      auto* img = Gtk::make_managed<Gtk::Image>(pixbuf);\n      btn_box->pack_start(*img, false, false, 0);\n    }\n    std::string label_text = title;\n    if (label_text.empty() || label_text == app_id) {\n      label_text = manager_.getRewrite(app_id, title);\n      if (label_text.empty() || label_text == "?" || label_text == app_id) {\n        label_text = "?";\n      }\n    }\n    const std::string separator = " - ";\n    const auto first_separator = label_text.find(separator);\n    const auto last_separator = label_text.rfind(separator);\n    if (first_separator != std::string::npos &&\n        last_separator != first_separator &&\n        last_separator > first_separator + separator.length()) {\n      const std::string outer_prefix = label_text.substr(0, first_separator);\n      const std::string outer_suffix = label_text.substr(last_separator + separator.length());\n      if (!outer_prefix.empty() && outer_prefix == outer_suffix) {\n        label_text = label_text.substr(\n            first_separator + separator.length(),\n            last_separator - first_separator - separator.length());\n      }\n    }\n    if (label_text.length() > 20) {\n      label_text = label_text.substr(0, 20);\n    }\n    auto* lbl = Gtk::make_managed<Gtk::Label>(label_text);\n    lbl->set_ellipsize(Pango::ELLIPSIZE_END);\n    lbl->set_single_line_mode(true);\n    int title_len = (int)label_text.length();\n    if (title_len > 20) title_len = 20;\n    int min_chars = (title_len >= 10) ? (title_len - 2) : 1;\n    lbl->set_width_chars(min_chars);\n    btn_box->pack_start(*lbl, true, true, 0);\n    btn->add(*btn_box);/' \
  src/modules/niri/workspace.cpp
