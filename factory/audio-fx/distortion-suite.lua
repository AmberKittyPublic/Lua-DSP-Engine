-- Multi-mode distortion suite.
-- Macro 1: model, 2: drive, 3: tone, 4: mix, 5: output,
-- 6: bias, 7: character, 8: bit depth, 9: downsample, 10: low cut.

local models = {
    "Saturator",
    "Bitcrush / Downsample",
    "Tube",
    "Tape",
    "Overdrive",
    "Fuzz",
    "Diode",
}

local model_scale = { type = "int", min = 0, max = 6 }
local drive_scale = { type = "float", min = 0.0, max = 36.0 }
local tone_scale = { type = "float", min = 0.0, max = 1.0 }
local mix_scale = { type = "float", min = 0.0, max = 1.0 }
local output_scale = { type = "float", min = -12.0, max = 12.0 }
local bias_scale = { type = "float", min = -1.0, max = 1.0 }
local character_scale = { type = "float", min = 0.0, max = 1.0 }
local bits_scale = { type = "int", min = 1, max = 16 }
local downsample_scale = { type = "int", min = 1, max = 32 }
local low_cut_scale = { type = "float", min = 20.0, max = 500.0 }

params.set_range(1, model_scale)
params.set_range(2, drive_scale)
params.set_range(3, tone_scale)
params.set_range(4, mix_scale)
params.set_range(5, output_scale)
params.set_range(6, bias_scale)
params.set_range(7, character_scale)
params.set_range(8, bits_scale)
params.set_range(9, downsample_scale)
params.set_range(10, low_cut_scale)

params.set_defaults({
    [1] = 0,
    [2] = 6.0,
    [3] = 1.0,
    [4] = 0.5,
    [5] = 0.0,
    [6] = 0.0,
    [7] = 0.35,
    [8] = 8,
    [9] = 1,
    [10] = 20.0,
})

local low_state = {}
local high_state = {}
local held_sample = {}
local sample_counter = {}

local function clamp(value, low, high)
    return math.max(low, math.min(high, value))
end

local function quantize(value, bits)
    local levels = 2.0 ^ (bits - 1)
    local clipped = clamp(value, -1.0, 1.0)
    return math.floor(clipped * levels + 0.5) / levels
end

local function shape_sample(model, value, bias, character, bits)
    if model == 0 then
        return math.tanh((value + bias * 0.15) * (1.0 + character * 4.0))
    elseif model == 1 then
        return quantize(value, bits)
    elseif model == 2 then
        local asymmetric = value + bias * 0.2
        if asymmetric >= 0.0 then
            return math.tanh(asymmetric * (1.2 + character * 2.0))
        end
        return math.tanh(asymmetric * (0.75 + character * 1.25))
    elseif model == 3 then
        local soft = math.tanh((value + bias * 0.1) * 1.35)
        local compressed = clamp(value, -1.0, 1.0)
        return soft * (1.0 - character * 0.25) + compressed * character * 0.25
    elseif model == 4 then
        local threshold = 1.0 - character * 0.9
        return clamp(value, -threshold, threshold) / threshold
    elseif model == 5 then
        local fuzz = value * (2.0 + character * 12.0) + bias * 0.2
        if math.abs(fuzz) < 0.015 then
            return 0.0
        end
        return math.tanh(fuzz)
    end

    local diode = value + bias * 0.15
    local threshold = 0.05 + character * 0.35
    local excess = math.abs(diode) - threshold
    if excess <= 0.0 then
        return 0.0
    end
    local polarity = diode < 0.0 and -1.0 or 1.0
    return polarity * (1.0 - math.exp(-excess * (3.0 + character * 15.0)))
end

function process(num_samples, sample_rate, tempo, playing, pos_beats)
    if sample_rate <= 0.0 or num_samples <= 0 then
        return
    end

    local channels = audio.num_channels()
    local model = math.floor(params.get(1) + 0.5)
    local drive_db = params.get(2)
    local tone = params.get(3)
    local mix = params.get(4)
    local output_db = params.get(5)
    local bias = params.get(6)
    local character = params.get(7)
    local bits = params.get(8)
    local downsample = params.get(9)
    local low_cut = params.get(10)

    local drive = 10.0 ^ (drive_db / 20.0)
    local output = 10.0 ^ (output_db / 20.0)
    local cutoff = 500.0 * (2.0 ^ (tone * 5.3219))
    cutoff = math.min(cutoff, sample_rate * 0.45)
    local low_alpha = tone >= 0.999 and 1.0
        or 1.0 - math.exp(-2.0 * math.pi * cutoff / sample_rate)
    local high_alpha = math.exp(-2.0 * math.pi * low_cut / sample_rate)
    downsample = math.max(1, math.floor(downsample + 0.5))

    for channel = 0, channels - 1 do
        low_state[channel] = low_state[channel] or 0.0
        high_state[channel] = high_state[channel] or 0.0
        held_sample[channel] = held_sample[channel] or 0.0
        sample_counter[channel] = sample_counter[channel] or 0
    end

    for sample = 0, num_samples - 1 do
        for channel = 0, channels - 1 do
            local input = audio.get(channel, sample) or 0.0
            local highpassed = input - high_state[channel]
            high_state[channel] = high_state[channel] * high_alpha + input * (1.0 - high_alpha)
            local driven = highpassed * drive
            local shaped

            if model == 1 then
                if sample_counter[channel] == 0 then
                    held_sample[channel] = shape_sample(model, driven, bias, character, bits)
                end
                shaped = held_sample[channel]
                sample_counter[channel] = (sample_counter[channel] + 1) % downsample
            else
                shaped = shape_sample(model, driven, bias, character, bits)
            end

            local previous = low_state[channel]
            local filtered = previous + low_alpha * (shaped - previous)
            low_state[channel] = filtered
            local wet = filtered * output
            audio.set(channel, sample, input * (1.0 - mix) + wet * mix)
        end
    end
end

function draw()
    local model = math.floor(params.get(1) + 0.5)
    local drive = 10.0 ^ (params.get(2) / 20.0)
    local bias = params.get(6)
    local character = params.get(7)
    local bits = params.get(8)
    local output = 10.0 ^ (params.get(5) / 20.0)

    ui.colored_heading("Distortion Suite", "orange")
    ui.label("Seven character modes, from soft saturation to digital reduction")
    ui.radio_param(1, "Model", models)

    ui.row(function()
        ui.column(function()
            ui.group("Level", function()
                ui.row(function()
                    ui.knob(2, "Drive", drive_scale)
                    ui.knob(5, "Output", output_scale)
                    ui.knob(4, "Mix", mix_scale)
                end)
            end)
            ui.group("Tone and character", function()
                ui.row(function()
                    ui.knob(3, "Tone", tone_scale)
                    ui.knob(10, "Low cut", low_cut_scale)
                    ui.knob(6, "Bias", bias_scale)
                    ui.knob(7, "Character", character_scale)
                end)
            end)
            ui.group("Digital reduction", function()
                ui.row(function()
                    ui.knob(8, "Bit depth", bits_scale)
                    ui.knob(9, "Downsample", downsample_scale)
                end)
            end)
        end)

        ui.graph(function()
            local curve = {}
            for index = 0, 128 do
                local input = -1.0 + 2.0 * index / 128.0
                local value = shape_sample(model, input * drive, bias, character, bits) * output
                curve[index + 1] = (clamp(value, -1.0, 1.0) + 1.0) * 0.5
            end
            ui.plot(curve, "orange")
        end)
    end)

    ui.separator_horizontal()
    ui.meter(host.meter(), "output peak")
end
