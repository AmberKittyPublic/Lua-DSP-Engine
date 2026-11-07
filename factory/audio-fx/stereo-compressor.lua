-- Stereo peak compressor with an optional external sidechain.
-- Macro 1: threshold, Macro 2: ratio, Macro 3: attack, Macro 4: release,
-- Macro 5: makeup gain, Macro 6: dry/wet, Macro 7: external sidechain blend,
-- Macro 8: knee, Macro 9: compressor character.

local characters = { "Clean", "Glue", "Transistor", "Tube", "Optical" }
local character_descriptions = {
    "Transparent peak compression",
    "RMS-style detection with a softer knee",
    "Fast response, firmer knee, and light saturation",
    "Slower adaptive release with gentle saturation",
    "Slower attack and smooth, even compression",
}

params.set_defaults({
    [1] = 0.625,
    [2] = 0.333,
    [3] = 0.2,
    [4] = 0.18,
    [5] = 0.1,
    [6] = 1.0,
    [7] = 0.0,
    [8] = 0.25,
    [9] = 0,
})

local knee_scale = { type = "float", min = 0.0, max = 12.0 }
local character_scale = { type = "int", min = 0, max = 4 }
params.set_range(8, knee_scale)
params.set_range(9, character_scale)

local envelope = 0.0
local rms_envelope = 0.0
local previous_character = 0

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

function process(num_samples, sample_rate, tempo, playing, pos_beats)
    if sample_rate <= 0.0 or num_samples <= 0 then
        return
    end

    local channels = audio.num_channels()
    local sidechain_channels = audio.sc_num_channels()
    local sidechain_blend = params.get(7)
    if sidechain_channels <= 0 then
        sidechain_blend = 0.0
    end
    local threshold_db = -48.0 + params.get(1) * 48.0
    local ratio = 1.0 + params.get(2) * 9.0
    local attack_seconds = (0.1 + params.get(3) * 49.9) / 1000.0
    local release_seconds = (10.0 + params.get(4) * 490.0) / 1000.0
    local makeup = 10.0 ^ (params.get(5) * 12.0 / 20.0)
    local mix = params.get(6)
    local character = math.floor(params.get(9) + 0.5)
    local knee_db = shaped_knee(params.get(8), character)
    ratio = shaped_ratio(ratio, character)
    if character ~= previous_character then
        if character == 1 then
            rms_envelope = envelope * envelope
        elseif previous_character == 1 then
            envelope = math.sqrt(math.max(0.0, rms_envelope))
        end
        previous_character = character
    end

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

    local attack_coefficient = math.exp(-1.0 / (attack_seconds * sample_rate))
    local release_coefficient = math.exp(-1.0 / (release_seconds * sample_rate))
    local reduction_sum = 0.0

    for sample = 0, num_samples - 1 do
        local main_peak = 0.0
        for channel = 0, channels - 1 do
            local input = audio.get(channel, sample) or 0.0
            main_peak = math.max(main_peak, math.abs(input))
        end

        local detector = main_peak
        if sidechain_blend > 0.0 then
            local sidechain_peak = 0.0
            for channel = 0, sidechain_channels - 1 do
                sidechain_peak = math.max(
                    sidechain_peak,
                    math.abs(audio.sc_get(channel, sample) or 0.0)
                )
            end
            detector = main_peak * (1.0 - sidechain_blend) + sidechain_peak * sidechain_blend
        end

        if character == 1 then
            local detector_power = detector * detector
            local coefficient = detector_power > rms_envelope
                and attack_coefficient or release_coefficient
            rms_envelope = coefficient * rms_envelope + (1.0 - coefficient) * detector_power
            envelope = math.sqrt(math.max(0.0, rms_envelope))
        else
            local coefficient = detector > envelope and attack_coefficient or release_coefficient
            if character == 3 and detector < envelope then
                local level_db = 20.0 * math.log(math.max(envelope, 1e-9)) / math.log(10.0)
                if level_db > threshold_db then
                    coefficient = math.exp(-1.0 / (release_seconds * 1.5 * sample_rate))
                end
            end
            envelope = coefficient * envelope + (1.0 - coefficient) * detector
        end

        local detector_db = 20.0 * math.log(math.max(envelope, 1e-9)) / math.log(10.0)
        local reduction_db = reduction_at(detector_db, threshold_db, ratio, knee_db)
        local compressed_gain = 10.0 ^ (-reduction_db / 20.0) * makeup
        reduction_sum = reduction_sum + reduction_db

        for channel = 0, channels - 1 do
            local input = audio.get(channel, sample) or 0.0
            local compressed = input * compressed_gain
            if character == 2 then
                compressed = math.tanh(compressed * 1.08) / 1.08
            elseif character == 3 then
                compressed = math.tanh(compressed * 1.12) / 1.12
            end
            audio.set(channel, sample, input * (1.0 - mix) + compressed * mix)
        end
    end

    host.trace(reduction_sum / math.max(1, num_samples) / 24.0)
end

function draw()
    ui.heading("Stereo Compressor")
    ui.label("Linked broadband compression with external sidechain blend")
    ui.radio_param(9, "Character", characters)
    ui.label(character_descriptions[math.floor(params.get(9) + 0.5) + 1])

    ui.group("Levels", function()
        ui.row(function()
            ui.column(function()
                ui.vertical_meter(host.input_channel_meter(0))
                ui.vertical_meter(host.input_channel_meter(1))
            end)
            ui.graph(function()
                ui.plot(host.level_trace(), "green")
                ui.plot(host.gain_trace(), "red")

                local curve = {}
                local threshold = -48.0 + params.get(1) * 48.0
                local character = math.floor(params.get(9) + 0.5)
                local ratio = shaped_ratio(1.0 + params.get(2) * 9.0, character)
                local knee = shaped_knee(params.get(8), character)
                for i = 0, 128 do
                    local input = i / 128.0
                    local input_db = -60.0 + input * 60.0
                    local reduction = reduction_at(input_db, threshold, ratio, knee)
                    local output_db = input_db - reduction
                    local output = math.max(0.0, math.min(1.0, (output_db + 60.0) / 60.0))
                    curve[i + 1] = output
                end
                ui.plot(curve, "orange")
            end)
            ui.column(function()
                ui.vertical_meter(host.output_channel_meter(0))
                ui.vertical_meter(host.output_channel_meter(1))
            end)
        end)
    end)

    ui.row(function()
        ui.group("Compression", function()
            ui.row(function()
                ui.knob(1, "Threshold")
                ui.knob(2, "Ratio")
                ui.knob(8, "Knee", knee_scale)
            end)
            ui.row(function()
                ui.knob(3, "Attack")
                ui.knob(4, "Release")
            end)
        end)

        ui.group("Output", function()
            ui.row(function()
                ui.knob(5, "Makeup")
                ui.knob(6, "Mix")
            end)
        end)
    end)

    ui.separator_horizontal()
    ui.knob(7, "External sidechain blend")
    ui.label("0 uses the main input; 1 uses the sidechain input.")
end
