-- Three-band compressor with independent dynamics per band and an optional sidechain.
-- Macros 1-2: low/high crossover, 3: character, 4: sidechain blend, 5: mix,
-- 6: output, 7-12: low band, 13-18: mid band, 19-24: high band.

local characters = { "Clean", "Glue", "Transistor", "Tube", "Optical" }
local character_descriptions = {
    "Transparent peak compression",
    "RMS-style detection with a softer knee",
    "Fast response, firmer knee, and light saturation",
    "Slower adaptive release with gentle saturation",
    "Slower attack and smooth, even compression",
}
local colors = { "#61d6c3", "orange", "#c678dd" }

local scales = {
    [1] = { type = "float", min = 50.0, max = 500.0 },
    [2] = { type = "float", min = 500.0, max = 8000.0 },
    [3] = { type = "int", min = 0, max = 4 },
    [4] = { type = "float", min = 0.0, max = 1.0 },
    [5] = { type = "float", min = 0.0, max = 1.0 },
    [6] = { type = "float", min = -12.0, max = 12.0 },
}

local bands = {
    { name = "Low", threshold = 7, ratio = 8, attack = 9, release = 10, knee = 11, makeup = 12 },
    { name = "Mid", threshold = 13, ratio = 14, attack = 15, release = 16, knee = 17, makeup = 18 },
    { name = "High", threshold = 19, ratio = 20, attack = 21, release = 22, knee = 23, makeup = 24 },
}

for _, band in ipairs(bands) do
    scales[band.threshold] = { type = "float", min = -48.0, max = 0.0 }
    scales[band.ratio] = { type = "float", min = 1.0, max = 20.0 }
    scales[band.attack] = { type = "float", min = 0.1, max = 100.0 }
    scales[band.release] = { type = "float", min = 10.0, max = 1000.0 }
    scales[band.knee] = { type = "float", min = 0.0, max = 12.0 }
    scales[band.makeup] = { type = "float", min = -12.0, max = 12.0 }
end

for index, scale in pairs(scales) do
    params.set_range(index, scale)
end

params.set_defaults({
    [1] = 250.0,
    [2] = 2500.0,
    [3] = 0,
    [4] = 0.0,
    [5] = 1.0,
    [6] = 0.0,
    [7] = -18.0, [8] = 3.0, [9] = 20.0, [10] = 180.0, [11] = 4.0, [12] = 0.0,
    [13] = -20.0, [14] = 2.5, [15] = 12.0, [16] = 140.0, [17] = 3.0, [18] = 0.0,
    [19] = -24.0, [20] = 2.0, [21] = 4.0, [22] = 90.0, [23] = 2.0, [24] = 0.0,
})

local envelopes = { 0.0, 0.0, 0.0 }
local rms_envelopes = { 0.0, 0.0, 0.0 }
local previous_character = 0
local low_states, mid_states = {}, {}
local sidechain_low_states, sidechain_mid_states = {}, {}
local main_bands = { {}, {}, {} }
local main_peaks = { 0.0, 0.0, 0.0 }
local sidechain_peaks = { 0.0, 0.0, 0.0 }
local gains = { 1.0, 1.0, 1.0 }

local function split_sample(input, channel, low_state, mid_state, low_alpha, mid_alpha)
    local low = low_state[channel] or 0.0
    low = low + low_alpha * (input - low)
    low_state[channel] = low

    local high_passed = input - low
    local mid = mid_state[channel] or 0.0
    mid = mid + mid_alpha * (high_passed - mid)
    mid_state[channel] = mid

    return low, mid, high_passed - mid
end

local function shaped_knee(knee_db, character)
    if character == 1 then
        return knee_db + 4.0
    elseif character == 2 then
        return knee_db * 0.2
    elseif character == 3 then
        return knee_db + 2.0
    elseif character == 4 then
        return knee_db + 4.0
    end
    return knee_db
end

local function shaped_ratio(ratio, character)
    if character == 2 then
        return math.min(20.0, ratio * 1.2)
    end
    return ratio
end

local function reduction_at(detector_db, threshold_db, ratio, knee_db)
    if knee_db > 0.0 and detector_db > threshold_db - knee_db / 2.0
        and detector_db < threshold_db + knee_db / 2.0 then
        local x = detector_db - threshold_db + knee_db / 2.0
        return (1.0 - 1.0 / ratio) * x * x / (2.0 * knee_db)
    elseif detector_db >= threshold_db + knee_db / 2.0 then
        return detector_db - threshold_db - (detector_db - threshold_db) / ratio
    end
    return 0.0
end

local function make_band_config(band, character, sample_rate)
    local ratio = shaped_ratio(params.get(band.ratio), character)
    local attack_seconds = params.get(band.attack) / 1000.0
    local release_seconds = params.get(band.release) / 1000.0

    if character == 1 then
        attack_seconds = attack_seconds * 1.2
        release_seconds = release_seconds * 1.25
    elseif character == 2 then
        attack_seconds = math.max(0.00005, attack_seconds * 0.35)
        release_seconds = release_seconds * 0.8
    elseif character == 3 then
        attack_seconds = attack_seconds * 1.5
        release_seconds = release_seconds * 2.0
    elseif character == 4 then
        attack_seconds = attack_seconds * 3.0
        release_seconds = release_seconds * 1.75
    end

    return {
        threshold = params.get(band.threshold),
        ratio = ratio,
        knee = shaped_knee(params.get(band.knee), character),
        attack = math.exp(-1.0 / (attack_seconds * sample_rate)),
        release = math.exp(-1.0 / (release_seconds * sample_rate)),
        tube_release = math.exp(-1.0 / (release_seconds * 1.5 * sample_rate)),
        makeup = 10.0 ^ (params.get(band.makeup) / 20.0),
    }
end

function process(num_samples, sample_rate, tempo, playing, pos_beats)
    if sample_rate <= 0.0 or num_samples <= 0 then
        return
    end

    local channels = audio.num_channels()
    local sidechain_channels = audio.sc_num_channels()
    local sidechain_blend = sidechain_channels > 0 and params.get(4) or 0.0
    local character = math.floor(params.get(3) + 0.5)
    local mix = params.get(5)
    local output_gain = 10.0 ^ (params.get(6) / 20.0)
    local configs = {}

    if character ~= previous_character then
        for band = 1, 3 do
            if character == 1 then
                rms_envelopes[band] = envelopes[band] * envelopes[band]
            elseif previous_character == 1 then
                envelopes[band] = math.sqrt(math.max(0.0, rms_envelopes[band]))
            end
        end
        previous_character = character
    end

    for band = 1, 3 do
        configs[band] = make_band_config(bands[band], character, sample_rate)
    end

    local low_crossover = math.min(params.get(1), sample_rate * 0.2)
    local high_crossover = math.min(math.max(params.get(2), low_crossover * 1.25), sample_rate * 0.45)
    local low_alpha = 1.0 - math.exp(-2.0 * math.pi * low_crossover / sample_rate)
    local mid_alpha = 1.0 - math.exp(-2.0 * math.pi * high_crossover / sample_rate)
    local reduction_sum = 0.0

    for sample = 0, num_samples - 1 do
        main_peaks[1], main_peaks[2], main_peaks[3] = 0.0, 0.0, 0.0
        sidechain_peaks[1], sidechain_peaks[2], sidechain_peaks[3] = 0.0, 0.0, 0.0

        for channel = 0, channels - 1 do
            local input = audio.get(channel, sample) or 0.0
            local low, mid, high = split_sample(
                input, channel, low_states, mid_states, low_alpha, mid_alpha
            )
            main_bands[1][channel] = low
            main_bands[2][channel] = mid
            main_bands[3][channel] = high
            main_peaks[1] = math.max(main_peaks[1], math.abs(low))
            main_peaks[2] = math.max(main_peaks[2], math.abs(mid))
            main_peaks[3] = math.max(main_peaks[3], math.abs(high))
        end

        if sidechain_blend > 0.0 then
            for channel = 0, sidechain_channels - 1 do
                local input = audio.sc_get(channel, sample) or 0.0
                local low, mid, high = split_sample(
                    input,
                    channel,
                    sidechain_low_states,
                    sidechain_mid_states,
                    low_alpha,
                    mid_alpha
                )
                sidechain_peaks[1] = math.max(sidechain_peaks[1], math.abs(low))
                sidechain_peaks[2] = math.max(sidechain_peaks[2], math.abs(mid))
                sidechain_peaks[3] = math.max(sidechain_peaks[3], math.abs(high))
            end
        end

        for band = 1, 3 do
            local config = configs[band]
            local detector = main_peaks[band] * (1.0 - sidechain_blend)
                + sidechain_peaks[band] * sidechain_blend
            if character == 1 then
                local detector_power = detector * detector
                local coefficient = detector_power > rms_envelopes[band]
                    and config.attack or config.release
                rms_envelopes[band] = coefficient * rms_envelopes[band]
                    + (1.0 - coefficient) * detector_power
                envelopes[band] = math.sqrt(math.max(0.0, rms_envelopes[band]))
            else
                local coefficient = detector > envelopes[band]
                    and config.attack or config.release
                if character == 3 and detector < envelopes[band] then
                    local level_db = 20.0 * math.log(math.max(envelopes[band], 1e-9)) / math.log(10.0)
                    if level_db > config.threshold then
                        coefficient = config.tube_release
                    end
                end
                envelopes[band] = coefficient * envelopes[band]
                    + (1.0 - coefficient) * detector
            end

            local detector_db = 20.0 * math.log(math.max(envelopes[band], 1e-9)) / math.log(10.0)
            local reduction_db = reduction_at(
                detector_db, config.threshold, config.ratio, config.knee
            )
            gains[band] = 10.0 ^ (-reduction_db / 20.0) * config.makeup
            reduction_sum = reduction_sum + reduction_db / 3.0
        end

        for channel = 0, channels - 1 do
            local input = audio.get(channel, sample) or 0.0
            local compressed = main_bands[1][channel] * gains[1]
                + main_bands[2][channel] * gains[2]
                + main_bands[3][channel] * gains[3]
            if character == 2 then
                compressed = math.tanh(compressed * 1.08) / 1.08
            elseif character == 3 then
                compressed = math.tanh(compressed * 1.12) / 1.12
            end
            local output = input * (1.0 - mix) + compressed * mix
            audio.set(channel, sample, output * output_gain)
        end
    end

    host.trace(reduction_sum / num_samples / 24.0)
end

function draw()
    ui.heading("Three-Band Compressor")
    ui.label("Complementary low, mid, and high bands with independent compression")
    ui.radio_param(3, "Character", characters)
    ui.label(character_descriptions[math.floor(params.get(3) + 0.5) + 1])

    ui.row(function()
        ui.knob(1, "Low crossover", scales[1])
        ui.knob(2, "High crossover", scales[2])
        ui.knob(4, "Sidechain blend", scales[4])
        ui.knob(5, "Mix", scales[5])
        ui.knob(6, "Output", scales[6])
    end)
    local low_crossover = math.min(params.get(1), host.sample_rate * 0.2)
    local high_crossover = math.min(math.max(params.get(2), low_crossover * 1.25), host.sample_rate * 0.45)
    ui.label(string.format("Active crossovers: %.0f Hz / %.0f Hz", low_crossover, high_crossover))

    ui.group("Levels and transfer curves", function()
        ui.row(function()
            ui.column(function()
                ui.vertical_meter(host.input_channel_meter(0))
                ui.vertical_meter(host.input_channel_meter(1))
            end)
            ui.graph(function()
                ui.plot(host.level_trace(), "green")
                ui.plot(host.gain_trace(), "red")
                for band_index, band in ipairs(bands) do
                    local curve = {}
                    local character = math.floor(params.get(3) + 0.5)
                    local threshold = params.get(band.threshold)
                    local ratio = shaped_ratio(params.get(band.ratio), character)
                    local knee = shaped_knee(params.get(band.knee), character)
                    for index = 0, 128 do
                        local input = index / 128.0
                        local input_db = -60.0 + input * 60.0
                        local reduction = reduction_at(input_db, threshold, ratio, knee)
                        local output_db = input_db - reduction
                        curve[index + 1] = math.max(0.0, math.min(1.0, (output_db + 60.0) / 60.0))
                    end
                    ui.plot(curve, colors[band_index])
                end
            end)
            ui.column(function()
                ui.vertical_meter(host.output_channel_meter(0))
                ui.vertical_meter(host.output_channel_meter(1))
            end)
        end)
    end)

    for band_index, band in ipairs(bands) do
        ui.colored_heading(band.name .. " band", colors[band_index])
        ui.group(band.name .. " band", function()
            ui.row(function()
                ui.knob(band.threshold, "Threshold", scales[band.threshold])
                ui.knob(band.ratio, "Ratio", scales[band.ratio])
                ui.knob(band.attack, "Attack", scales[band.attack])
                ui.knob(band.release, "Release", scales[band.release])
                ui.knob(band.knee, "Knee", scales[band.knee])
                ui.knob(band.makeup, "Makeup", scales[band.makeup])
            end)
        end)
    end
end
