-- Stereo peak compressor with an optional external sidechain.
-- Macro 1: threshold, Macro 2: ratio, Macro 3: attack, Macro 4: release,
-- Macro 5: makeup gain, Macro 6: dry/wet, Macro 7: external sidechain blend.

params.set_defaults({
    [1] = 0.625,
    [2] = 0.333,
    [3] = 0.2,
    [4] = 0.18,
    [5] = 0.1,
    [6] = 1.0,
    [7] = 0.0,
})

local envelope = 0.0

function process(num_samples, sample_rate, tempo, playing, pos_beats)
    if sample_rate <= 0.0 then
        return
    end

    local channels = audio.num_channels()
    local sidechain_channels = audio.sc_num_channels()
    local sidechain_blend = params.get(7)
    local threshold_db = -48.0 + params.get(1) * 48.0
    local ratio = 1.0 + params.get(2) * 9.0
    local attack_seconds = (0.1 + params.get(3) * 49.9) / 1000.0
    local release_seconds = (10.0 + params.get(4) * 490.0) / 1000.0
    local attack_coefficient = math.exp(-1.0 / (attack_seconds * sample_rate))
    local release_coefficient = math.exp(-1.0 / (release_seconds * sample_rate))
    local makeup = 10.0 ^ (params.get(5) * 12.0 / 20.0)
    local mix = params.get(6)

    for sample = 0, num_samples - 1 do
        local main_peak = 0.0
        for channel = 0, channels - 1 do
            main_peak = math.max(main_peak, math.abs(audio.get(channel, sample) or 0.0))
        end

        local detector = main_peak
        if sidechain_blend > 0.0 and sidechain_channels > 0 then
            local sidechain_peak = 0.0
            for channel = 0, sidechain_channels - 1 do
                sidechain_peak = math.max(
                    sidechain_peak,
                    math.abs(audio.sc_get(channel, sample) or 0.0)
                )
            end
            detector = main_peak * (1.0 - sidechain_blend) + sidechain_peak * sidechain_blend
        end

        local coefficient = detector > envelope and attack_coefficient or release_coefficient
        envelope = coefficient * envelope + (1.0 - coefficient) * detector

        local detector_db = 20.0 * math.log(math.max(envelope, 1e-9)) / math.log(10.0)
        local gain_reduction_db = 0.0
        if detector_db > threshold_db then
            local compressed_db = threshold_db + (detector_db - threshold_db) / ratio
            gain_reduction_db = compressed_db - detector_db
        end
        local compressed_gain = 10.0 ^ ((gain_reduction_db / 20.0)) * makeup
        local wet_gain = (1.0 - mix) + mix * compressed_gain

        for channel = 0, channels - 1 do
            local input = audio.get(channel, sample) or 0.0
            audio.set(channel, sample, input * wet_gain)
        end
    end
end

function draw()
    ui.heading("Stereo Compressor")
    ui.label("Linked peak detector with external sidechain blend")

    ui.row(function()
        ui.param(1, "Threshold")
        ui.param(2, "Ratio")
    end)
    ui.row(function()
        ui.param(3, "Attack")
        ui.param(4, "Release")
    end)
    ui.row(function()
        ui.param(5, "Makeup")
        ui.param(6, "Mix")
    end)

    ui.separator_horizontal()
    ui.param(7, "External sidechain blend")
    ui.label("0 uses the main input; 1 uses the sidechain input.")
    ui.meter(host.meter(), "output peak")
end
