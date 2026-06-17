-- vsn1-cli runtime bundle 2026-06-12-screen-first.1
--
-- This bundle captures the first standalone runtime contract using the owned
-- LCD slots that were confirmed by the POC:
-- - page 0 / element 13 / event 0  : LCD init
-- - page 0 / element 13 / event 8  : LCD draw
--
-- The state shape below is grounded in the validated update_param(...)
-- arguments plus the layered field names accepted by the implementation plan.

vsn1_cli_bundle_version = "2026-06-12-screen-first.1"
vsn1_cli_runtime_marker = "vsn1-cli:2026-06-12-screen-first.1:lcd-init"

vsn1_cli_state = vsn1_cli_state or {
  persistent = {
    title = "",
    bottom = "",
    value = 0,
    min = 0,
    max = 127,
    default = -1,
    step = 0,
    info = {"---", "---", "---", "---", "---", "---", "---", "---"},
    clamp_min = 0,
    clamp_max = 0,
    bank = 0
  },
  slow = {
    message = ""
  },
  fast = {
    action = ""
  }
}

function vsn1_cli_runtime_identity()
  return vsn1_cli_runtime_marker
end

local function vsn1_cli_normalize_text(value)
  return tostring(value or "")
end

local function vsn1_cli_normalize_int(value, fallback)
  local number = tonumber(value)
  if number == nil then
    return fallback
  end

  return math.floor(number)
end

local function vsn1_cli_copy_info(info)
  local result = {"---", "---", "---", "---", "---", "---", "---", "---"}
  if type(info) ~= "table" then
    return result
  end

  for index = 1, 8 do
    result[index] = vsn1_cli_normalize_text(info[index] or result[index])
  end

  return result
end

function vsn1_cli_set_field(name, value)
  if name == "persistent.title" then
    vsn1_cli_state.persistent.title = vsn1_cli_normalize_text(value)
  elseif name == "persistent.bottom" then
    vsn1_cli_state.persistent.bottom = vsn1_cli_normalize_text(value)
  elseif name == "persistent.value" then
    vsn1_cli_state.persistent.value = vsn1_cli_normalize_int(value, 0)
  elseif name == "persistent.min" then
    vsn1_cli_state.persistent.min = vsn1_cli_normalize_int(value, 0)
  elseif name == "persistent.max" then
    vsn1_cli_state.persistent.max = vsn1_cli_normalize_int(value, 127)
  elseif name == "persistent.default" then
    vsn1_cli_state.persistent.default = vsn1_cli_normalize_int(value, -1)
  elseif name == "persistent.step" then
    vsn1_cli_state.persistent.step = vsn1_cli_normalize_int(value, 0)
  elseif name == "persistent.info" then
    vsn1_cli_state.persistent.info = vsn1_cli_copy_info(value)
  elseif name == "persistent.clamp_min" then
    vsn1_cli_state.persistent.clamp_min = value and 1 or 0
  elseif name == "persistent.clamp_max" then
    vsn1_cli_state.persistent.clamp_max = value and 1 or 0
  elseif name == "persistent.bank" then
    vsn1_cli_state.persistent.bank = vsn1_cli_normalize_int(value, 0)
  elseif name == "slow.message" then
    vsn1_cli_state.slow.message = vsn1_cli_normalize_text(value)
  elseif name == "fast.action" then
    vsn1_cli_state.fast.action = vsn1_cli_normalize_text(value)
  else
    error("unknown vsn1-cli field: " .. tostring(name))
  end
end

function vsn1_cli_clear_layer(layer)
  if layer == "persistent" then
    vsn1_cli_state.persistent.title = ""
    vsn1_cli_state.persistent.bottom = ""
    vsn1_cli_state.persistent.value = 0
    vsn1_cli_state.persistent.min = 0
    vsn1_cli_state.persistent.max = 127
    vsn1_cli_state.persistent.default = -1
    vsn1_cli_state.persistent.step = 0
    vsn1_cli_state.persistent.info = {"---", "---", "---", "---", "---", "---", "---", "---"}
    vsn1_cli_state.persistent.clamp_min = 0
    vsn1_cli_state.persistent.clamp_max = 0
    vsn1_cli_state.persistent.bank = 0
  elseif layer == "slow" then
    vsn1_cli_state.slow.message = ""
  elseif layer == "fast" then
    vsn1_cli_state.fast.action = ""
  else
    error("unknown vsn1-cli layer: " .. tostring(layer))
  end
end

function vsn1_cli_activate_layer(layer)
  if layer ~= "slow" and layer ~= "fast" then
    error("unsupported activation layer: " .. tostring(layer))
  end

  vsn1_cli_state.active_layer = layer
end

function update_param(value, min_value, max_value, title, bottom_text, step_indicator, default_value, info, clamps, bank)
  vsn1_cli_set_field("persistent.value", value)
  vsn1_cli_set_field("persistent.min", min_value)
  vsn1_cli_set_field("persistent.max", max_value)
  vsn1_cli_set_field("persistent.title", title)
  vsn1_cli_set_field("persistent.bottom", bottom_text)
  vsn1_cli_set_field("persistent.step", step_indicator)
  vsn1_cli_set_field("persistent.default", default_value)
  vsn1_cli_set_field("persistent.info", info)
  vsn1_cli_set_field("persistent.bank", bank)

  if type(clamps) == "table" then
    vsn1_cli_set_field("persistent.clamp_min", clamps[1])
    vsn1_cli_set_field("persistent.clamp_max", clamps[2])
  else
    vsn1_cli_set_field("persistent.clamp_min", 0)
    vsn1_cli_set_field("persistent.clamp_max", 0)
  end
end

function vsn1_cli_snapshot()
  return vsn1_cli_state
end
