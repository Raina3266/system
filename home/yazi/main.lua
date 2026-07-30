-- Yazi init.lua — plugin setup.
-- Colors mirror theme.nix (Cyberpunk palette).
local bg = "#0a0a14"
local bgmod = "#0E0616"
local fg = "#cbe3e7"
local pink = "#ff7edb"
local cyan = "#7afcff"
local dim = "#5c6776"

-- ── UI ─────────────────────────────────────────────────────────────────────
require("full-border"):setup()
require("git"):setup { order = 1500 }
require("githead"):setup()
require("starship"):setup()
require("easyjump"):setup { icon_fg = cyan, first_key_fg = dim }

-- Header line stays Yazi's default (starship + githead render into it).
require("yatline"):setup {
  show_background = false,
  display_header_line = false,
  style_a = {
    fg = bg,
    bg_mode = { normal = pink, select = cyan, un_set = dim },
  },
  style_b = { bg = bgmod, fg = fg },
  style_c = { bg = bg, fg = fg },
  permissions_t_fg = cyan,
  permissions_r_fg = "#f29e74",
  permissions_w_fg = "#ff3333",
  permissions_x_fg = pink,
  permissions_s_fg = fg,
  header_line = {},
  status_line = {
    left = {
      section_a = {
        { type = "string", name = "tab_mode" },
      },
      section_b = {
        { type = "string", name = "hovered_size" },
      },
      section_c = {
        { type = "string", name = "hovered_name" },
        { type = "coloreds", name = "count" },
      },
    },
    right = {
      section_a = {
        { type = "string", name = "cursor_position" },
      },
      section_b = {
        { type = "string", name = "cursor_percentage" },
      },
      section_c = {
        { type = "string", name = "hovered_file_extension", params = { true } },
        { type = "coloreds", name = "permissions" },
      },
    },
  },
}

-- ── File manipulation ──────────────────────────────────────────────────────
require("smart-enter"):setup { open_multi = true }
require("bookmarks"):setup()
require("batch-rename-gui"):setup { editor = "zeditor" }
require("recycle-bin"):setup()
require("yafg"):setup { editor = "nvim" }

-- ── Previewers ─────────────────────────────────────────────────────────────
require("duckdb"):setup()
require("allmytoes"):setup { sizes = { "n", "l", "x", "xx" } }
