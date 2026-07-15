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
#   4. Buttons shrink when overflowing; short titles shrink more than
#      long titles (long titles get a higher minimum width).
#   5. Render icon + text title in each taskbar button. Deduplicates
#      redundant title segments ("App - Page - App" -> "Page") and
#      strips the app name from the title's start/end. Adds padding
#      around the icon and title.

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
# share the available space (combined with the min-width logic in #5,
# short titles shrink more than long titles).
sed -i 's|taskbar_box_.pack_start(\*btn, false, false, 0);|taskbar_box_.pack_start(*btn, true, true, 0);|' \
  src/modules/niri/workspace.cpp

# 5. Render icon + text title in each taskbar button.
# Upstream shows either an icon OR a 3-char fallback. Replace with:
# an icon (if available) followed by the window title (truncated to
# 20 chars). Before truncation, the title is cleaned up:
#   a) Deduplicate "X - Y - X" patterns (common with Chrome PWAs that
#      prepend+append the app name around the page title).
#   b) Strip the rewritten app name from the title's start/end
#      (handles "AppName - RealTitle" patterns).
#   c) Strip the raw app_id from the title's start/end.
# If the title is empty or becomes empty after stripping, fall back
# to the rewritten app name, then to "?".
# The label minimum width scales with title length: titles >= 10 chars
# get min (length - 2) so they barely shrink; shorter titles get min
# 1 char so they compress first when overflowing.
# Padding: box spacing 6px between icon and title, 4px margin on each
# side of the button content.
perl -0777 -i -pe 's/    auto pixbuf = loadIcon\(app_id, icon_size\);\n    if \(pixbuf\) \{\n      auto\* img = Gtk::make_managed<Gtk::Image>\(pixbuf\);\n      btn->add\(\*img\);\n    \} else \{\n      std::string fallback = app_id.empty\(\) \? title : app_id;\n      if \(!fallback.empty\(\)\) \{\n        fallback = fallback.substr\(0, 3\);\n      \} else \{\n        fallback = "\?";\n      \}\n      auto\* lbl = Gtk::make_managed<Gtk::Label>\(fallback\);\n      btn->add\(\*lbl\);\n    \}/    auto* btn_box = Gtk::make_managed<Gtk::Box>(Gtk::ORIENTATION_HORIZONTAL, 6);\n    btn_box->set_halign(Gtk::ALIGN_CENTER);\n    btn_box->set_margin_start(4);\n    btn_box->set_margin_end(4);\n    auto pixbuf = loadIcon(app_id, icon_size);\n    if (pixbuf) {\n      auto* img = Gtk::make_managed<Gtk::Image>(pixbuf);\n      btn_box->pack_start(*img, false, false, 0);\n    }\n    std::string label_text = title;\n    std::string delim = " - ";\n    size_t first_sep = label_text.find(delim);\n    if (first_sep != std::string::npos) {\n      size_t last_sep = label_text.rfind(delim);\n      if (last_sep != first_sep) {\n        std::string head = label_text.substr(0, first_sep);\n        std::string tail = label_text.substr(last_sep + delim.length());\n        if (head == tail) {\n          label_text = label_text.substr(first_sep + delim.length(), last_sep - first_sep - delim.length());\n        }\n      }\n    }\n    std::string app_name = manager_.getRewrite(app_id, title);\n    if (app_name.empty() || app_name == "?") app_name = app_id;\n    std::string prefix = app_name + " - ";\n    if (label_text.length() > prefix.length() && label_text.substr(0, prefix.length()) == prefix) {\n      label_text = label_text.substr(prefix.length());\n    }\n    std::string suffix = " - " + app_name;\n    if (label_text.length() > suffix.length() && label_text.substr(label_text.length() - suffix.length()) == suffix) {\n      label_text = label_text.substr(0, label_text.length() - suffix.length());\n    }\n    prefix = app_id + " - ";\n    if (label_text.length() > prefix.length() && label_text.substr(0, prefix.length()) == prefix) {\n      label_text = label_text.substr(prefix.length());\n    }\n    suffix = " - " + app_id;\n    if (label_text.length() > suffix.length() && label_text.substr(label_text.length() - suffix.length()) == suffix) {\n      label_text = label_text.substr(0, label_text.length() - suffix.length());\n    }\n    if (label_text.empty() || label_text == app_id) {\n      label_text = app_name;\n      if (label_text.empty() || label_text == "?") {\n        label_text = app_id.empty() ? "?" : app_id;\n      }\n    }\n    if (label_text.length() > 20) {\n      label_text = label_text.substr(0, 20);\n    }\n    auto* lbl = Gtk::make_managed<Gtk::Label>(label_text);\n    lbl->set_ellipsize(Pango::ELLIPSIZE_END);\n    lbl->set_single_line_mode(true);\n    int title_len = (int)label_text.length();\n    if (title_len > 20) title_len = 20;\n    int min_chars = (title_len >= 10) ? (title_len - 2) : 1;\n    lbl->set_width_chars(min_chars);\n    btn_box->pack_start(*lbl, true, true, 0);\n    btn->add(*btn_box);/' \
  src/modules/niri/workspace.cpp
