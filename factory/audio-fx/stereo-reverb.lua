-- Stereo algorithmic reverb with mode-shaped comb and diffusion networks.
-- Macros 1-4 retain their original decay, damping, wet, and width mappings.
-- Macros 5-10: character, predelay, size/diffusion, modulation, early reflections, freeze.

local modes = {
    {
        name = "Room",
        color = "#69c9b5",
        combs = { 0.0297, 0.0371, 0.0411, 0.0437 },
        right_combs = { 0.0307, 0.0389, 0.0421, 0.0449 },
        allpasses = { 0.0050, 0.0017, 0.0011 },
        right_allpasses = { 0.0053, 0.0019 },
        allpass_count = 2,
        diffusion_out = 1.0,
        diffusion_store = 0.5,
        weights = { 1.00, 1.00, 1.00, 1.00 },
        weight_sum = 4.00,
        decay = 1.00,
        diffusion = 0.52,
        lfo_rate = 0.13,
        tap_times = { 0.008, 0.017, 0.029 },
        tap_weights = { 0.70, 0.42, 0.24 },
    },
    {
        name = "Hall",
        color = "#8c9eff",
        combs = { 0.0431, 0.0537, 0.0611, 0.0719 },
        allpasses = { 0.0071, 0.0031, 0.0019 },
        weights = { 1.00, 1.00, 1.00, 1.00 },
        weight_sum = 4.00,
        decay = 1.02,
        diffusion = 0.62,
        lfo_rate = 0.09,
        tap_times = { 0.014, 0.027, 0.041 },
        tap_weights = { 0.54, 0.38, 0.27 },
    },
    {
        name = "Plate",
        color = "#f2a6cf",
        combs = { 0.0223, 0.0299, 0.0347, 0.0401 },
        allpasses = { 0.0043, 0.0015, 0.0009 },
        weights = { 1.10, 1.02, 0.94, 0.86 },
        weight_sum = 3.92,
        decay = 0.97,
        diffusion = 0.72,
        lfo_rate = 0.19,
        tap_times = { 0.006, 0.014, 0.025 },
        tap_weights = { 0.82, 0.48, 0.30 },
    },
    {
        name = "Chamber",
        color = "#e5bb78",
        combs = { 0.0313, 0.0407, 0.0491, 0.0583 },
        allpasses = { 0.0083, 0.0039, 0.0021 },
        weights = { 0.92, 1.08, 1.06, 0.94 },
        weight_sum = 4.00,
        decay = 0.99,
        diffusion = 0.64,
        lfo_rate = 0.11,
        tap_times = { 0.011, 0.023, 0.037 },
        tap_weights = { 0.64, 0.45, 0.31 },
    },
    {
        name = "Spring",
        color = "#f39b62",
        combs = { 0.0173, 0.0239, 0.0311, 0.0397 },
        allpasses = { 0.0033, 0.0013, 0.0008 },
        weights = { 1.26, 1.02, 0.78, 0.56 },
        weight_sum = 3.62,
        decay = 0.91,
        diffusion = 0.38,
        lfo_rate = 0.23,
        tap_times = { 0.004, 0.010, 0.020 },
        tap_weights = { 0.96, 0.54, 0.30 },
    },
}

local mode_names = { "Room", "Hall", "Plate", "Chamber", "Spring" }
local character_scale = { type = "int", min = 0, max = 4 }
local predelay_scale = { type = "float", min = 0.0, max = 250.0 }
local size_scale = { type = "float", min = 1.0, max = 1.35 }
local modulation_scale = { type = "float", min = 0.0, max = 1.0 }
local early_scale = { type = "float", min = 0.0, max = 1.0 }

params.set_range(5, character_scale)
params.set_range(6, predelay_scale)
params.set_range(7, size_scale)
params.set_range(8, modulation_scale)
params.set_range(9, early_scale)

params.set_defaults({
    [1] = 0.55,
    [2] = 0.35,
    [3] = 0.3,
    [4] = 0.85,
    [5] = 0,
    [6] = 18.0,
    [7] = 1.0,
    [8] = 0.12,
    [9] = 0.25,
    [10] = 0,
})

local function make_delay(max_seconds, sample_rate)
    local length = math.max(2, math.floor(max_seconds * sample_rate + 0.5) + 2)
    local buffer = {}
    for index = 1, length do
        buffer[index] = 0.0
    end
    return { buffer = buffer, index = 1, damped = 0.0 }
end

local function make_bank(sample_rate, stereo_offset)
    local bank = { combs = {}, allpasses = {} }
    for index = 1, 4 do
        bank.combs[index] = make_delay(0.12 * stereo_offset, sample_rate)
    end
    for index = 1, 3 do
        bank.allpasses[index] = make_delay(0.025 * stereo_offset, sample_rate)
    end
    return bank
end

local left = make_bank(host.sample_rate, 1.0)
local right = make_bank(host.sample_rate, 1.013)
local predelay_left = make_delay(0.31, host.sample_rate)
local predelay_right = make_delay(0.31, host.sample_rate)
local modulation_phase = 0.0

local function read_delay(delay, distance)
    local read_index = delay.index - distance
    if read_index <= 0 then
        read_index = read_index + #delay.buffer
    end
    if read_index > #delay.buffer then
        read_index = read_index - #delay.buffer
    end
    return delay.buffer[read_index]
end

local function process_bank(bank, input, feedback, damping, comb_lengths, allpass_lengths,
        modulation_offset, mode)
    local sum = 0.0
    for index = 1, 4 do
        local comb = bank.combs[index]
        local distance = comb_lengths[index]
        if index == 1 then
            distance = math.max(1, distance + modulation_offset)
        end
        local delayed = read_delay(comb, distance)
        comb.damped = delayed * (1.0 - damping) + comb.damped * damping
        comb.buffer[comb.index] = input + comb.damped * feedback
        comb.index = comb.index + 1
        if comb.index > #comb.buffer then
            comb.index = 1
        end
        sum = sum + delayed * mode.weights[index]
    end

    local signal = sum / mode.weight_sum
    local allpass_count = mode.allpass_count or 3
    for index = 1, allpass_count do
        local diffuser = bank.allpasses[index]
        local delayed = read_delay(diffuser, allpass_lengths[index])
        local output = delayed - signal * (mode.diffusion_out or mode.diffusion)
        diffuser.buffer[diffuser.index] = signal + delayed * (mode.diffusion_store or mode.diffusion)
        diffuser.index = diffuser.index + 1
        if diffuser.index > #diffuser.buffer then
            diffuser.index = 1
        end
        signal = output
    end
    return signal
end

function process(num_samples, sample_rate, tempo, playing, pos_beats)
    if sample_rate <= 0.0 or num_samples <= 0 then
        return
    end

    local channels = audio.num_channels()
    if channels <= 0 then
        return
    end

    local decay = params.get(1)
    local damping = params.get(2) * 0.95
    local wet = params.get(3)
    local width = params.get(4)
    local mode_index = math.max(1, math.min(5, math.floor(params.get(5) + 0.5) + 1))
    local mode = modes[mode_index]
    local predelay_samples = math.floor(params.get(6) * sample_rate / 1000.0 + 0.5)
    local size = params.get(7)
    local modulation = params.get(8)
    local early = params.get(9)
    local freeze = params.get(10) >= 0.5
    local stereo = channels > 1

    local comb_lengths_left = {}
    local comb_lengths_right = {}
    local allpass_lengths_left = {}
    local allpass_lengths_right = {}
    for index = 1, 4 do
        local length = math.max(1, math.floor(mode.combs[index] * size * sample_rate + 0.5))
        comb_lengths_left[index] = length
        local right_length = mode.right_combs and mode.right_combs[index] or mode.combs[index] * 1.013
        comb_lengths_right[index] = math.max(1, math.floor(right_length * size * sample_rate + 0.5))
    end
    local allpass_count = mode.allpass_count or 3
    for index = 1, allpass_count do
        local length = math.max(1, math.floor(mode.allpasses[index] * size * sample_rate + 0.5))
        allpass_lengths_left[index] = length
        local right_length = mode.right_allpasses and mode.right_allpasses[index]
            or mode.allpasses[index] * 1.013
        allpass_lengths_right[index] = math.max(1, math.floor(right_length * size * sample_rate + 0.5))
    end

    local feedback = (0.55 + decay * 0.4) * mode.decay * (0.9 + size * 0.1)
    feedback = math.min(0.985, feedback)
    if freeze then
        feedback = 0.9997
        damping = 0.0
    end

    local predelay_read = math.max(1, predelay_samples)
    local early_delay_1 = math.max(1, predelay_samples + math.floor(mode.tap_times[1] * sample_rate + 0.5))
    local early_delay_2 = math.max(1, predelay_samples + math.floor(mode.tap_times[2] * sample_rate + 0.5))
    local early_delay_3 = math.max(1, predelay_samples + math.floor(mode.tap_times[3] * sample_rate + 0.5))
    local modulation_depth = math.floor(modulation * 0.0015 * sample_rate + 0.5)
    local phase_step = 2.0 * math.pi * mode.lfo_rate / sample_rate

    for sample = 0, num_samples - 1 do
        local input_left = audio.get(0, sample) or 0.0
        local input_right = stereo and (audio.get(1, sample) or input_left) or input_left
        local excite_left = input_left + input_right * 0.15
        local excite_right = input_right + input_left * 0.15

        local delayed_left = predelay_samples == 0 and excite_left or read_delay(predelay_left, predelay_read)
        local delayed_right = predelay_samples == 0 and excite_right or read_delay(predelay_right, predelay_read)
        local early_left = 0.0
        local early_right = 0.0
        if early > 0.0 and not freeze then
            early_left = read_delay(predelay_left, early_delay_1) * mode.tap_weights[1]
                + read_delay(predelay_left, early_delay_2) * mode.tap_weights[2]
                + read_delay(predelay_left, early_delay_3) * mode.tap_weights[3]
            early_right = read_delay(predelay_right, early_delay_1) * mode.tap_weights[1]
                + read_delay(predelay_right, early_delay_2) * mode.tap_weights[2]
                + read_delay(predelay_right, early_delay_3) * mode.tap_weights[3]
        end

        predelay_left.buffer[predelay_left.index] = excite_left
        predelay_left.index = predelay_left.index + 1
        if predelay_left.index > #predelay_left.buffer then
            predelay_left.index = 1
        end
        predelay_right.buffer[predelay_right.index] = excite_right
        predelay_right.index = predelay_right.index + 1
        if predelay_right.index > #predelay_right.buffer then
            predelay_right.index = 1
        end

        if freeze then
            delayed_left = 0.0
            delayed_right = 0.0
        end
        local mod_offset = math.floor(math.sin(modulation_phase) * modulation_depth + 0.5)
        modulation_phase = modulation_phase + phase_step
        if modulation_phase >= 2.0 * math.pi then
            modulation_phase = modulation_phase - 2.0 * math.pi
        end

        local wet_left = process_bank(left, delayed_left, feedback, damping,
            comb_lengths_left, allpass_lengths_left, mod_offset, mode)
        local wet_right = wet_left
        if stereo then
            wet_right = process_bank(right, delayed_right, feedback, damping,
                comb_lengths_right, allpass_lengths_right, -mod_offset, mode)
            local mid = (wet_left + wet_right) * 0.5
            local side = (wet_left - wet_right) * 0.5 * width
            wet_left = mid + side
            wet_right = mid - side
            local early_mid = (early_left + early_right) * 0.5
            local early_side = (early_left - early_right) * 0.5 * width
            early_left = early_mid + early_side
            early_right = early_mid - early_side
            audio.set(0, sample, input_left * (1.0 - wet) + (wet_left + early_left * early) * wet)
            audio.set(1, sample, input_right * (1.0 - wet) + (wet_right + early_right * early) * wet)
        else
            audio.set(0, sample, input_left * (1.0 - wet) + (wet_left + early_left * early) * wet)
        end
    end
end

function draw()
    local mode_index = math.max(1, math.min(5, math.floor(params.get(5) + 0.5) + 1))
    local mode = modes[mode_index]
    local decay = (0.55 + params.get(1) * 0.4) * mode.decay
    local tail = {}
    local reflections = {}
    for index = 0, 96 do
        local position = index / 96.0
        tail[index + 1] = math.max(0.02, decay ^ (position * 6.0))
        local pulse = 0.06
        if index == 7 or index == 18 or index == 34 then
            pulse = 0.42 + params.get(9) * 0.48
        end
        reflections[index + 1] = pulse
    end

    ui.colored_heading("STEREO REVERB / " .. string.upper(mode.name), mode.color)
    ui.label("Mode-shaped space, diffusion, and stereo modulation")
    ui.radio_param(5, "Character", mode_names)
    ui.label("" .. mode.name .. " | " .. (mode_index == 1 and "compact, natural" or
        mode_index == 2 and "long, spacious" or mode_index == 3 and "bright, dense" or
        mode_index == 4 and "smooth, dimensional" or "resonant, springy"))

    ui.row(function()
        ui.column(function()
            ui.group("Core reverb", function()
                ui.row(function()
                    ui.knob(1, "Decay")
                    ui.knob(2, "Damping")
                    ui.knob(3, "Wet")
                    ui.knob(4, "Width")
                end)
            end)
            ui.group("Space and motion", function()
                ui.row(function()
                    ui.knob(6, "Predelay", predelay_scale)
                    ui.knob(7, "Size / diffusion", size_scale)
                    ui.knob(8, "Modulation", modulation_scale)
                    ui.knob(9, "Early reflections", early_scale)
                end)
                ui.toggle(10, "Freeze tail")
            end)
        end)
        ui.column(function()
            ui.group("Tail profile", function()
                ui.graph(function()
                    ui.plot(tail, mode.color)
                    ui.plot(reflections, "#f3bd72")
                end)
                ui.label("Late-field decay and early reflection energy")
            end)
            ui.group("Signal", function()
                ui.colored_meter(math.min(1.0, host.input_meter()), "Input", "#69c9b5")
                ui.colored_meter(math.min(1.0, host.meter()), "Output", mode.color)
            end)
        end)
    end)
end
