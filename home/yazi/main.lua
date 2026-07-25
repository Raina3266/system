-- Yazi main.lua — plugin setup.

-- ── 1. Smart File Manipulation & Integration ─────────────────────────────
require("full-border"):setup()
require("duckdb"):setup()
require("smart-enter"):setup { open_multi = true }
require("bookmarks"):setup {}

-- ── 2. UI Customization & Quality of Life ────────────────────────────────
require("git"):setup { order = 1500 }
require("githead"):setup()
require("starship"):setup()

-- ── 3. File Operations & Integrations ────────────────────────────────────
require("recycle-bin"):setup()
require("yafg"):setup { editor = "nvim" }

