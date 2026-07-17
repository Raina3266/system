#!/nix/store/xxxx-bash/bin/bash
# Patches applied to Waybar's niri/workspaces module at build time.
# These modify src/modules/niri/workspace.cpp and
# include/modules/niri/workspaces.hpp before compilation.
#
# Patches:
#   1. Make getRewrite() public so Workspace can call it for text labels.
#   2. Fix hide-empty to work together with current-only.
#   3. Hide the taskbar box when there are no windows
#   4. Buttons shrink when overflowing.
#   5. Render icon + text title (20 chars) in each taskbar button.

set -e

# 1. Make getRewrite public so Workspace can call it for text labels.
sed -i 's|  std::string getRewrite(const std::string\& app_id, const std::string\& title);||' \
  include/modules/niri/workspaces.hpp
sed -i 's|  std::string getIcon(const std::string\& value, const Json::Value\& ws) const;|&\n  std::string getRewrite(const std::string\& app_id, const std::string\& title);|' \
  include/modules/niri/workspaces.hpp

# 2. Fix visibility: make hide-empty work together with current-only.
perl -0777 -i -pe 's/    data\[prop\]\.asBool\(\) \? button_\.show\(\) : button_\.hide\(\);/    bool should_show = data[prop].asBool();\n    if (should_show \&\& cfg["hide-empty"].asBool() \&\& data["active_window_id"].isNull() \&\& !data["is_focused"].asBool()) {\n      should_show = false;\n    }\n    should_show ? button_.show() : button_.hide();/' \
  src/modules/niri/workspace.cpp

# 3. Hide the taskbar box when there are no windows on the workspace.
perl -0777 -i -pe 's/    rebuildTaskbar\(my_windows\);\n    taskbar_box_\.show\(\);\n    label_\.hide\(\);/    rebuildTaskbar(my_windows);\n    if (my_windows.empty()) {\n      taskbar_box_.hide();\n      label_.hide();\n    } else {\n      box_.set_hexpand(false);\n      box_.set_halign(Gtk::ALIGN_FILL);\n      taskbar_box_.set_hexpand(false);\n      taskbar_box_.set_halign(Gtk::ALIGN_FILL);\n      taskbar_box_.show();\n      label_.hide();\n    }/' \
  src/modules/niri/workspace.cpp

# 4. Let taskbar buttons shrink when the bar overflows.
sed -i 's|taskbar_box_.pack_start(\*btn, false, false, 0);|taskbar_box_.pack_start(*btn, false, true, 0);|' \
  src/modules/niri/workspace.cpp

# 5. Render icon + text title in each taskbar button.
cat > /tmp/waybar-replacement.txt << 'ENDREPLACEMENT'
    auto* btn_box = Gtk::make_managed<Gtk::Box>(Gtk::ORIENTATION_HORIZONTAL, 0);
    btn_box->set_halign(Gtk::ALIGN_FILL);
    btn_box->set_hexpand(false);
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
    auto ci_eq = [](const std::string& a, const std::string& b) {
      if (a.size() != b.size()) return false;
      for (size_t i = 0; i < a.size(); i++)
        if (std::tolower(a[i]) != std::tolower(b[i])) return false;
      return true;
    };
    std::string rewrite = manager_.getRewrite(app_id, title);
    auto strip_segment = [&](const std::string& name) {
      if (name.empty()) return;
      std::string prefix = name + separator;
      if (label_text.length() > prefix.length() &&
          ci_eq(label_text.substr(0, prefix.length()), prefix)) {
        label_text = label_text.substr(prefix.length());
        return;
      }
      std::string suffix = separator + name;
      if (label_text.length() > suffix.length() &&
          ci_eq(label_text.substr(label_text.length() - suffix.length()), suffix)) {
        label_text = label_text.substr(0, label_text.length() - suffix.length());
      }
    };
    strip_segment(app_id);
    if (!rewrite.empty() && rewrite != "?" && rewrite != app_id) {
      strip_segment(rewrite);
    }
    int char_count = 0;
    size_t trunc_pos = 0;
    while (trunc_pos < label_text.size() && char_count < 20) {
      unsigned char c = label_text[trunc_pos];
      if (c < 0x80) trunc_pos += 1;
      else if ((c & 0xE0) == 0xC0) trunc_pos += 2;
      else if ((c & 0xF0) == 0xE0) trunc_pos += 3;
      else if ((c & 0xF8) == 0xF0) trunc_pos += 4;
      else trunc_pos += 1;
      char_count++;
    }
    if (trunc_pos < label_text.size()) {
      label_text = label_text.substr(0, trunc_pos);
    }
    auto* lbl = Gtk::make_managed<Gtk::Label>(label_text);
    lbl->set_ellipsize(Pango::ELLIPSIZE_END);
    lbl->set_xalign(0.0);
    lbl->set_single_line_mode(true);
    int min_chars = char_count;
    if (min_chars > 20) min_chars = 20;
    if (min_chars < 5) min_chars = 5;
    lbl->set_width_chars(min_chars);
    btn_box->pack_start(*lbl, false, false, 0);
    btn->add(*btn_box);
ENDREPLACEMENT

REPLACEMENT=$(cat /tmp/waybar-replacement.txt)
rm /tmp/waybar-replacement.txt

perl -0777 -i -pe "s/    auto pixbuf = loadIcon\\(app_id, icon_size\\);\n    if \\(pixbuf\\) \\{\\n      auto\\* img = Gtk::make_managed<Gtk::Image>\\(pixbuf\\);\\n      btn->add\\(\\*img\\);\\n    \\} else \\{\\n      std::string fallback = app_id.empty\\(\\) \\? title : app_id;\\n      if \\(!fallback.empty\\(\\)\\) \\{\\n        fallback = fallback.substr\\(0, 3\\);\\n      \\} else \\{\\n        fallback = \"\\?\";\\n      \\}\\n      auto\\* lbl = Gtk::make_managed<Gtk::Label>\\(fallback\\);\\n      btn->add\\(\\*lbl\\);\\n    \\}/$REPLACEMENT/" \
  src/modules/niri/workspace.cpp
