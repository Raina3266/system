#!/nix/store/xxxx-bash/bin/bash
# Patches applied to Waybar's niri/workspaces module at build time.
# These modify src/modules/niri/workspace.cpp and
# include/modules/niri/workspaces.hpp before compilation.
#
# Patches:
#   1. Make getRewrite() public so Workspace can call it for text labels.
#   2. Fix hide-empty to work together with current-only.
#   3. Hide the taskbar box when there are no windows (no label re-show).
#      Box fills width (hexpand=true) so it can be clamped and shrink;
#      content left-aligned so few tabs don't stretch.
#   4. Buttons stay at natural size (no expand); overflow is handled
#      by wrapping taskbar_box_ in a Gtk::ScrolledWindow with horizontal
#      scrolling and no vertical scrolling (Option B). Few tabs render
#      left-aligned at natural width; many tabs scroll horizontally
#      instead of being squeezed to unreadable widths.
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

# 1b. Wrap taskbar_box_ in a Gtk::ScrolledWindow (Option B: scroll
# instead of squeeze). Add the scrolledwindow include and a new
# taskbar_scroll_ member declared before taskbar_box_.
sed -i 's|#include <gtkmm/label.h>|&\n#include <gtkmm/scrolledwindow.h>|' \
  include/modules/niri/workspace.hpp
# Insert taskbar_scroll_ member just before taskbar_box_. Match the
# member line regardless of trailing whitespace.
perl -0777 -i -pe 's/(  Gtk::Label label_;\s*\n)  Gtk::Box taskbar_box_;/${1}  Gtk::ScrolledWindow taskbar_scroll_;\n  Gtk::Box taskbar_box_;/' \
  include/modules/niri/workspace.hpp

# 2. Fix visibility: make hide-empty work together with current-only.
# Upstream uses if/else-if so current-only takes precedence and
# hide-empty is never checked. Replace the single-line ternary with
# a block that also checks hide-empty + active_window_id.
perl -0777 -i -pe 's/    data\[prop\]\.asBool\(\) \? button_\.show\(\) : button_\.hide\(\);/    bool should_show = data[prop].asBool();\n    if (should_show \&\& cfg["hide-empty"].asBool() \&\& data["active_window_id"].isNull() \&\& !data["is_focused"].asBool()) {\n      should_show = false;\n    }\n    should_show ? button_.show() : button_.hide();/' \
  src/modules/niri/workspace.cpp

# 3. Hide the taskbar scroll when there are no windows on the workspace.
# The ScrolledWindow is given hexpand=true so it can grow to fill the
# bar and scroll its contents horizontally when they overflow; halign=
# START keeps few-tab content left-aligned instead of stretching.
perl -0777 -i -pe 's/    rebuildTaskbar\(my_windows\);\n    taskbar_box_\.show\(\);\n    label_\.hide\(\);/    rebuildTaskbar(my_windows);\n    if (my_windows.empty()) {\n      taskbar_scroll_.hide();\n      label_.hide();\n    } else {\n      taskbar_scroll_.set_hexpand(true);\n      taskbar_scroll_.set_halign(Gtk::ALIGN_START);\n      taskbar_scroll_.show();\n      taskbar_box_.show();\n      label_.hide();\n    }/' \
  src/modules/niri/workspace.cpp

# 3b. Construct the ScrolledWindow in the initializer list and pack it
# into box_ instead of taskbar_box_ directly. Set horizontal scroll
# policy to AUTOMATIC and vertical to NEVER, and disable the scrollbar
# shadow/frame so it blends into the bar. taskbar_box_ becomes the
# child of the scrolled window.
perl -0777 -i -pe 's/      box_\(Gtk::ORIENTATION_HORIZONTAL, 0\),\n      taskbar_box_\(Gtk::ORIENTATION_HORIZONTAL, 0\) \{/      box_(Gtk::ORIENTATION_HORIZONTAL, 0),\n      taskbar_box_(Gtk::ORIENTATION_HORIZONTAL, 0) {/' \
  src/modules/niri/workspace.cpp
perl -0777 -i -pe 's/  button_\.add\(box_\);\n  box_\.pack_start\(label_, false, false, 0\);\n  box_\.pack_start\(taskbar_box_, false, false, 0\);/  button_.add(box_);\n  box_.pack_start(label_, false, false, 0);\n  taskbar_scroll_.set_policy(Gtk::POLICY_AUTOMATIC, Gtk::POLICY_NEVER);\n  taskbar_scroll_.set_propagate_natural_width(true);\n  taskbar_scroll_.set_hexpand(true);\n  taskbar_scroll_.set_halign(Gtk::ALIGN_START);\n  taskbar_scroll_.set_shadow_type(Gtk::SHADOW_NONE);\n  taskbar_scroll_.add(taskbar_box_);\n  box_.pack_start(taskbar_scroll_, true, true, 0);/' \
  src/modules/niri/workspace.cpp

# 3c. In the else branch (no windows), hide the scrolled window too
# so the empty taskbar doesn't reserve space. The child-clearing loop
# stays on taskbar_box_ since that's where buttons live.
perl -0777 -i -pe 's/  \} else \{\n    for \(auto\* child : taskbar_box_\.get_children\(\)\) \{\n      taskbar_box_\.remove\(\*child\);\n    \}\n    taskbar_box_\.hide\(\);\n  \}/  } else {\n    for (auto* child : taskbar_box_.get_children()) {\n      taskbar_box_.remove(*child);\n    }\n    taskbar_scroll_.hide();\n    taskbar_box_.hide();\n  }/' \
  src/modules/niri/workspace.cpp

# 4. Pack buttons without expand so they stay at natural size. The
# ScrolledWindow handles overflow by scrolling horizontally, so buttons
# no longer need to shrink toward minimums.
sed -i 's|taskbar_box_.pack_start(\*btn, false, false, 0);|taskbar_box_.pack_start(*btn, false, false, 0);|' \
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
# The label ellipsizes if it still overflows; since buttons are no
# longer forced to shrink (patch #4), width-chars shrinking is gone.
# Padding: box spacing 6px between icon and title, 4px margin on each
# side of the button content.
perl -0777 -i -pe 's/    auto pixbuf = loadIcon\(app_id, icon_size\);\n    if \(pixbuf\) \{\n      auto\* img = Gtk::make_managed<Gtk::Image>\(pixbuf\);\n      btn->add\(\*img\);\n    \} else \{\n      std::string fallback = app_id.empty\(\) \? title : app_id;\n      if \(!fallback.empty\(\)\) \{\n        fallback = fallback.substr\(0, 3\);\n      \} else \{\n        fallback = "\?";\n      \}\n      auto\* lbl = Gtk::make_managed<Gtk::Label>\(fallback\);\n      btn->add\(\*lbl\);\n    \}/    auto* btn_box = Gtk::make_managed<Gtk::Box>(Gtk::ORIENTATION_HORIZONTAL, 6);\n    btn_box->set_halign(Gtk::ALIGN_CENTER);\n    btn_box->set_margin_start(4);\n    btn_box->set_margin_end(4);\n    auto pixbuf = loadIcon(app_id, icon_size);\n    if (pixbuf) {\n      auto* img = Gtk::make_managed<Gtk::Image>(pixbuf);\n      btn_box->pack_start(*img, false, false, 0);\n    }\n    std::string label_text = title;\n    std::string delim = " - ";\n    size_t first_sep = label_text.find(delim);\n    if (first_sep != std::string::npos) {\n      size_t last_sep = label_text.rfind(delim);\n      if (last_sep != first_sep) {\n        std::string head = label_text.substr(0, first_sep);\n        std::string tail = label_text.substr(last_sep + delim.length());\n        if (head == tail) {\n          label_text = label_text.substr(first_sep + delim.length(), last_sep - first_sep - delim.length());\n        }\n      }\n    }\n    std::string app_name = manager_.getRewrite(app_id, title);\n    if (app_name.empty() || app_name == "?") app_name = app_id;\n    std::string prefix = app_name + " - ";\n    if (label_text.length() > prefix.length() && label_text.substr(0, prefix.length()) == prefix) {\n      label_text = label_text.substr(prefix.length());\n    }\n    std::string suffix = " - " + app_name;\n    if (label_text.length() > suffix.length() && label_text.substr(label_text.length() - suffix.length()) == suffix) {\n      label_text = label_text.substr(0, label_text.length() - suffix.length());\n    }\n    prefix = app_id + " - ";\n    if (label_text.length() > prefix.length() && label_text.substr(0, prefix.length()) == prefix) {\n      label_text = label_text.substr(prefix.length());\n    }\n    suffix = " - " + app_id;\n    if (label_text.length() > suffix.length() && label_text.substr(label_text.length() - suffix.length()) == suffix) {\n      label_text = label_text.substr(0, label_text.length() - suffix.length());\n    }\n    if (label_text.empty() || label_text == app_id) {\n      label_text = app_name;\n      if (label_text.empty() || label_text == "?") {\n        label_text = app_id.empty() ? "?" : app_id;\n      }\n    }\n    if (label_text.length() > 20) {\n      label_text = label_text.substr(0, 20);\n    }\n    auto* lbl = Gtk::make_managed<Gtk::Label>(label_text);\n    lbl->set_ellipsize(Pango::ELLIPSIZE_END);\n    lbl->set_single_line_mode(true);\n    btn_box->pack_start(*lbl, false, false, 0);\n    btn->add(*btn_box);/' \
  src/modules/niri/workspace.cpp
