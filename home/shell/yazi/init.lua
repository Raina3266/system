-- Yazi init.lua — plugin setup.
--
-- In yazi 26.5.6, plugins are activated by:
--   - Previewers/preloaders/fetchers: referenced from yazi.toml (no init.lua needed).
--   - Functional plugins: bound in keymap.toml, optionally configured here.
--
-- This file only contains setup() calls for plugins that need configuration.
-- See https://yazi-rs.github.io/docs/plugins/overview

-- ── 2. Smart File Manipulation & Integration ─────────────────────────────
require("full-border"):setup()
require("smart-enter"):setup { open_multi = true }
require("bookmarks"):setup {}

-- ── 3. UI Customization & Quality of Life ────────────────────────────────
require("git"):setup { order = 1500 }
require("githead"):setup()
require("relative-motions"):setup {
  show_numbers = "relative",
  show_motion = true,
  enter_mode = "first",
}
