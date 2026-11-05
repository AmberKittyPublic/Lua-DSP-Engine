-- Stereo Schroeder-style reverb using parallel feedback combs and serial diffusers.
-- Macro 1: room/decay, Macro 2: damping, Macro 3: wet mix, Macro 4: stereo width.

params.set_defaults({
    [1] = 0.55,
    [2] = 0.35,
    [3] = 0.3,
    [4] = 0.85,
})

local function make_delay(seconds, sample_rate)
    local length = math.max(1, math.floor(seconds * sample_rate + 0.5))
    local buffer = {}
    for index = 1, length do
        buffer[index] = 0.0
    end
    return { buffer = buffer, index = 1 }
end

local function make_bank(sample_rate, comb_times, allpass_times)
    local bank = { combs = {}, allpasses = {} }
    for _, seconds in ipairs(comb_times) do
        local delay = make_delay(seconds, sample_rate)
        delay.damped = 0.0
        bank.combs[#bank.combs + 1] = delay
    end
    for _, seconds in ipairs(allpass_times) do
        bank.allpasses[#bank.allpasses + 1] = make_delay(seconds, sample_rate)
    end
    return bank
end

local left = make_bank(
    host.sample_rate,
    { 0.0297, 0.0371, 0.0411, 0.0437 },
    { 0.0050, 0.0017 }
)
local right = make_bank(
    host.sample_rate,
    { 0.0307, 0.0389, 0.0421, 0.0449 },
    { 0.0053, 0.0019 }
)

local function process_bank(bank, input, feedback, damping)
    local sum = 0.0
    for _, comb in ipairs(bank.combs) do
        local delayed = comb.buffer[comb.index]
        comb.damped = delayed * (1.0 - damping) + comb.damped * damping
        comb.buffer[comb.index] = input + comb.damped * feedback
        comb.index = comb.index + 1
        if comb.index > #comb.buffer then
            comb.index = 1
        end
        sum = sum + delayed
    end

    local signal = sum / #bank.combs
    for _, diffuser in ipairs(bank.allpasses) do
        local delayed = diffuser.buffer[diffuser.index]
        local output = delayed - signal
        diffuser.buffer[diffuser.index] = signal + delayed * 0.5
        diffuser.index = diffuser.index + 1
        if diffuser.index > #diffuser.buffer then
            diffuser.index = 1
        end
        signal = output
    end
    return signal
end

function process(num_samples, sample_rate, tempo, playing, pos_beats)
    local channels = audio.num_channels()
    if channels == 0 then
        return
    end

    local feedback = 0.55 + params.get(1) * 0.4
    local damping = params.get(2) * 0.95
    local wet = params.get(3)
    local width = params.get(4)
    local stereo = channels > 1

    for sample = 0, num_samples - 1 do
        local input_left = audio.get(0, sample) or 0.0
        local input_right = stereo and (audio.get(1, sample) or input_left) or input_left
        local excite_left = input_left + input_right * 0.15
        local wet_left = process_bank(left, excite_left, feedback, damping)

        if stereo then
            local excite_right = input_right + input_left * 0.15
            local wet_right = process_bank(right, excite_right, feedback, damping)
            local mid = (wet_left + wet_right) * 0.5
            local side = (wet_left - wet_right) * 0.5 * width
            wet_left = mid + side
            wet_right = mid - side
            audio.set(0, sample, input_left * (1.0 - wet) + wet_left * wet)
            audio.set(1, sample, input_right * (1.0 - wet) + wet_right * wet)
        else
            audio.set(0, sample, input_left * (1.0 - wet) + wet_left * wet)
        end
    end
end

function draw()
    ui.heading("Stereo Reverb")
    ui.label("Four feedback combs and two allpass diffusers per channel")
    ui.row(function()
        ui.param(1, "Room / decay")
        ui.param(2, "Damping")
    end)
    ui.row(function()
        ui.param(3, "Wet")
        ui.param(4, "Width")
    end)
    ui.separator_horizontal()
    ui.meter(host.meter(), "output peak")
end
