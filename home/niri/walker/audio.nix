# Audio menu: list and control audio outputs/inputs via WirePlumber.
# Keybinds: Return=select default | Ctrl+Y/N=volume±5% | Ctrl+M=mute | Ctrl+T=toggle outputs/inputs
# Used by waybar speaker button. Improves on elephant wireplumber provider (au:) by grouping devices.
{ pkgs }:
''
  Name = "audio"
  NamePretty = "Audio Devices"
  Icon = "audio-card"
  Description = "Switch audio output/input device and set volume"
  HideFromProviderlist = false
  SearchName = true
  FixedOrder = true
  -- Fallback action (entries provide their own via Actions table)
  Action = "sh -c '%VALUE%'"

  local WPCTL = "${pkgs.wireplumber}/bin/wpctl"
  local STEP = "5%"

  -- Track outputs vs inputs mode (persists in elephant's per-menu state)
  local function showing_inputs()
    local s = state()
    return s ~= nil and s[1] == "inputs"
  end

  -- Toggle between outputs and inputs (Ctrl+T)
  function ToggleKind()
    if showing_inputs() then setState({ "outputs" }) else setState({ "inputs" }) end
  end

  -- Parse wpctl status between `first` and `last` section markers.
  -- Only first Audio tree (not Video) to avoid webcams appearing as audio inputs.
  local function devices(first, last)
    local list = {}
    local h = io.popen(WPCTL .. " status 2>/dev/null | sed -n '0,/" .. first
      .. ":/d; /" .. last .. ":/q; p'")
    if not h then return list end
    for line in h:lines() do
      -- Example line: " |  *   50. Built-in Audio Analog Stereo  [vol: 0.65]"
      local id = line:match("(%d+)%.")
      local desc = line:match("%d+%.%s+(.-)%s*%[vol:")
      local vol = line:match("%[vol: ([%d.]+)")
      if id and desc and vol then
        table.insert(list, {
          id = id,
          desc = desc,
          is_default = line:find("%*") ~= nil,
          -- MUTED in vol field indicates muted device
          muted = line:find("MUTED") ~= nil,
          volume = math.floor((tonumber(vol) or 0) * 100 + 0.5),
        })
      end
    end
    h:close()
    return list
  end

  -- Strip common prefix (usually card name) to show only distinguishing suffix
  -- (e.g., Speaker, Headphones, HDMI)
  local function trim_common_prefix(list)
    if #list < 2 then return end
    local words = {}
    for w in list[1].desc:gmatch("%S+") do
      table.insert(words, w)
    end
    local common = #words
    for i = 2, #list do
      local w2 = {}
      for w in list[i].desc:gmatch("%S+") do
        table.insert(w2, w)
      end
      local j = 0
      while j < common and j < #w2 and words[j + 1] == w2[j + 1] do
        j = j + 1
      end
      common = j
    end
    if common > 0 and common < #words then
      local prefix = table.concat(words, " ", 1, common) .. " "
      for _, d in ipairs(list) do
        if d.desc:sub(1, #prefix) == prefix then
          d.desc = d.desc:sub(#prefix + 1)
        end
      end
    end
  end

  -- Five-step volume bar visualization
  local function bar(volume)
    local filled = math.floor(volume / 20 + 0.5)
    if filled > 5 then filled = 5 end
    return string.rep("█", filled) .. string.rep("·", 5 - filled)
  end

  function GetEntries()
    local entries = {}
    local inputs = showing_inputs()
    local kind = inputs and "input" or "output"

    local list
    if inputs then
      list = devices("Sources", "Filters")
    else
      list = devices("Sinks", "Sources")
    end
    trim_common_prefix(list)

    for _, d in ipairs(list) do
      local marker = d.is_default and "✓" or " "
      local subtext
      if d.muted then
        subtext = "muted · " .. d.volume .. "%"
      else
        subtext = bar(d.volume) .. "  " .. d.volume .. "%"
      end

      local icon
      if inputs then
        icon = d.muted and "audio-input-microphone-muted" or "audio-input-microphone"
      else
        icon = d.muted and "audio-volume-muted" or "audio-volume-high"
      end

      table.insert(entries, {
        Text = marker .. "  " .. d.desc,
        Subtext = subtext,
        Icon = icon,
        Value = "true",
        Actions = {
          set_default = WPCTL .. " set-default " .. d.id,
          vol_up = WPCTL .. " set-volume -l 1.0 " .. d.id .. " " .. STEP .. "+",
          vol_down = WPCTL .. " set-volume " .. d.id .. " " .. STEP .. "-",
          toggle_mute = WPCTL .. " set-mute " .. d.id .. " toggle",
          toggle_kind = "lua:ToggleKind",
        },
      })
    end

    if #list == 0 then
      table.insert(entries, {
        Text = "No audio " .. kind .. "s found",
        Value = "true",
        Actions = { toggle_kind = "lua:ToggleKind" },
      })
    end

    return entries
  end
''
