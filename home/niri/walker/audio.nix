# Audio menu — lists both outputs (sinks) and inputs (sources) with
# their current volume, and allows switching the default device,
# adjusting per-device volume and toggling mute.
# Invoked via `walker -m menus:audio`.
#
# Return   select as default device
# ctrl y   volume +5%      ctrl n   volume -5%
# ctrl m   toggle mute     ctrl t   toggle outputs/inputs
#
# The elephant `wireplumber` provider (au:) covers similar ground,
# but keeps outputs and inputs in one flat list; this menu groups
# them and is what the waybar speaker button opens.
{ pkgs }:
''
  Name = "audio"
  NamePretty = "Audio Devices"
  Icon = "audio-card"
  Description = "Switch audio output/input device and set volume"
  HideFromProviderlist = false
  SearchName = true
  FixedOrder = true
  -- Unused: every entry supplies its command through its Actions
  -- table. Kept so an entry without actions would still do something.
  Action = "sh -c '%VALUE%'"

  local WPCTL = "${pkgs.wireplumber}/bin/wpctl"
  local STEP = "5%"

  -- Which of the two lists is showing. Mode lives in elephant's
  -- per-menu state, which persists across the throwaway Lua states
  -- used for each query.
  local function showing_inputs()
    local s = state()
    return s ~= nil and s[1] == "inputs"
  end

  -- Bound to the "toggle_kind" action (ctrl t).
  function ToggleKind()
    if showing_inputs() then setState({ "outputs" }) else setState({ "inputs" }) end
  end

  -- Parse one section of `wpctl status`. `first` and `last` delimit
  -- the block: sinks live between "Sinks:" and "Sources:", sources
  -- between "Sources:" and "Filters:".
  --
  -- `wpctl status` prints an Audio tree and then a Video tree, both
  -- with Sinks:/Sources:/Filters: headings, so only the first match
  -- of the range is taken (`0,/.../` + `q`) -- otherwise the webcam
  -- shows up as an audio input. Video entries carry no "[vol: ...]"
  -- either, so the volume match below is a second line of defence.
  local function devices(first, last)
    local list = {}
    local h = io.popen(WPCTL .. " status 2>/dev/null | sed -n '0,/" .. first
      .. ":/d; /" .. last .. ":/q; p'")
    if not h then return list end
    for line in h:lines() do
      -- e.g. " |  *   50. Built-in Audio Analog Stereo  [vol: 0.65]"
      local id = line:match("(%d+)%.")
      local desc = line:match("%d+%.%s+(.-)%s*%[vol:")
      local vol = line:match("%[vol: ([%d.]+)")
      if id and desc and vol then
        table.insert(list, {
          id = id,
          desc = desc,
          is_default = line:find("%*") ~= nil,
          -- wpctl marks a muted device with "MUTED" in the vol field.
          muted = line:find("MUTED") ~= nil,
          volume = math.floor((tonumber(vol) or 0) * 100 + 0.5),
        })
      end
    end
    h:close()
    return list
  end

  -- Strip the longest common word-prefix shared by all descriptions
  -- (usually the card name), so only the distinguishing suffix
  -- (Speaker, Headphones, HDMI ...) shows.
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

  -- A five-step bar giving the volume at a glance.
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
