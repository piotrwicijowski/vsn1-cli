local Module = {}

local function format_seconds(seconds)
    seconds = math.max(0, math.floor((tonumber(seconds) or 0) + 0.5))
    local hours = math.floor(seconds / 3600)
    local minutes = math.floor(seconds / 60) % 60
    local remainder = seconds % 60

    if hours > 0 then
        return ("%d:%02d:%02d"):format(hours, minutes, remainder)
    end

    return ("%d:%02d"):format(minutes, remainder)
end

function Module.init()
    glsb(255)
    vsn1_cli_state = {
        r = 1,
        w = "",
        b = "base",
        u = {},
        o = { "playback_status" },
        m = { base = 0, playback_status = 2 },
        l = {
            base = { a = "", t = "", l = "", d = 0, p = 0 },
            playback_status = { s = "" },
        },
    }
    c = c or {
        { 0, 0, 0 },
        { 255, 255, 255 },
        { 255, 160, 60 },
        { 160, 160, 160 },
        { 208, 208, 208 },
    }
end

function Module.set_field(layer, key, value)
    local fields = vsn1_cli_state.l[layer]
    if fields then
        fields[key] = value
        vsn1_cli_state.r = 1
    end
end

function Module.activate_layer(layer)
    local timeout = vsn1_cli_state.m[layer]
    if timeout == nil then
        return
    end

    if timeout > 0 then
        vsn1_cli_state.u[layer] = os.clock() + timeout
    else
        vsn1_cli_state.b = layer
    end

    vsn1_cli_state.r = 1
end

function Module.draw(self)
    local state = vsn1_cli_state
    local now = os.clock()
    local layer = state.b
    local layers = state.l
    local expiries = state.u

    for i = 1, #state.o do
        local candidate = state.o[i]
        if (expiries[candidate] or 0) > now then
            layer = candidate
        end
    end

    if state.r == 0 and state.w == layer then
        return
    end

    state.r = 0
    state.w = layer

    local base = layers.base
    local duration = base.d
    local progress = 0
    if duration > 0 then
        progress = base.p / duration
    end
    if progress < 0 then
        progress = 0
    elseif progress > 1 then
        progress = 1
    end

    self:ldaf(0, 0, 319, 239, c[1])
    self:ldft(base.a, 18, 18, 12, c[4])
    self:ldft(base.t, 18, 52, 16, c[2])
    self:ldft(base.l, 18, 86, 12, c[5])
    self:ldrr(18, 178, 301, 202, 8, c[2])
    local fill = 18 + math.floor(283 * progress)
    if fill > 18 then
        self:ldrrf(19, 179, fill, 201, 7, c[3])
    end
    self:ldft(format_seconds(base.p), 18, 212, 16, c[2])
    self:ldft(format_seconds(duration), 252, 212, 16, c[2])

    if layer == "playback_status" then
        local status = layers.playback_status.s
        if status ~= "" then
            self:ldaf(0, 0, 319, 239, c[1])
            self:ldft(status, 18, 111, 24, c[2])
        end
    end

    self:ldsw()
end

return Module
