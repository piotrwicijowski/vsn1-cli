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

local function draw_player_overlay(self, name)
    if name == "" then
        return
    end

    self:ldrrf(18, 112, 301, 150, 12, c[1])
    self:ldrr(18, 112, 301, 150, 12, c[2])
    self:ldft(name, 30, 124, 18, c[2])
end

local function draw_playback_status_overlay(self, status)
    if status == "" then
        return
    end

    local left = 116
    local top = 76
    local right = 204
    local bottom = 164
    local center_x = 160
    local center_y = 120
    local normalized = string.lower(status)

    self:ldrrf(left, top, right, bottom, 44, c[1])

    if normalized == "paused" then
        self:ldpof(
            { center_x - 18, center_x - 6, center_x - 6, center_x - 18 },
            { center_y - 24, center_y - 24, center_y + 24, center_y + 24 },
            c[2]
        )
        self:ldpof(
            { center_x + 6, center_x + 18, center_x + 18, center_x + 6 },
            { center_y - 24, center_y - 24, center_y + 24, center_y + 24 },
            c[2]
        )
        return
    end

    if normalized == "playing" then
        self:ldpof(
            { center_x - 16, center_x - 16, center_x + 18 },
            { center_y - 24, center_y + 24, center_y },
            c[2]
        )
        return
    end

    self:ldrr(82, 100, 238, 140, 10, c[2])
    self:ldft(status, 96, 111, 18, c[2])
end

function Module.init()
    glsb(255)
    vsn1_cli_state = {
        r = 1,
        w = "",
        b = "base",
        u = {},
        o = { "player", "playback_status" },
        m = { base = 0, player = 5, playback_status = 2 },
        l = {
            base = { a = "", t = "", l = "", d = 0, p = 0 },
            player = { n = "" },
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
    local layers = state.l
    local expiries = state.u
    local active = {}
    local signature = state.b

    for i = 1, #state.o do
        local candidate = state.o[i]
        if (expiries[candidate] or 0) > now then
            active[#active + 1] = candidate
        end
    end

    if #active > 0 then
        signature = signature .. ":" .. table.concat(active, ",")
    end

    if state.r == 0 and state.w == signature then
        return
    end

    state.r = 0
    state.w = signature

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

    for i = 1, #active do
        local layer = active[i]
        if layer == "player" then
            draw_player_overlay(self, layers.player.n)
        elseif layer == "playback_status" then
            draw_playback_status_overlay(self, layers.playback_status.s)
        end
    end

    self:ldsw()
end

return Module
