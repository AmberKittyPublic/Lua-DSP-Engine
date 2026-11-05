-- Stereo peak compressor with an optional external sidechain.
-- Macro 1: threshold, Macro 2: ratio, Macro 3: attack, Macro 4: release,
-- Macro 5: makeup gain, Macro 6: dry/wet, Macro 7: external sidechain blend,
-- Macro 8: knee.

params.set_defaults({
    [1] = 0.625,
    [2] = 0.333,
    [3] = 0.2,
    [4] = 0.18,
    [5] = 0.1,
    [6] = 1.0,
    [7] = 0.0,
    [8] = 0.25,
})

local knee_scale = { type = "float", min = 0.0, max = 12.0 }
params.set_range(8, knee_scale)

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
    local knee_db = params.get(8)

    local reduction_sum = 0.0

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
        local reduction_db = 0.0
        if detector_db > threshold_db + knee_db / 2.0 then
            reduction_db = detector_db - threshold_db - (detector_db - threshold_db) / ratio
        elseif detector_db > threshold_db - knee_db / 2.0 then
            local x = detector_db - threshold_db + knee_db / 2.0
            reduction_db = (1.0 - 1.0 / ratio) * x * x / (2.0 * knee_db)
        end
        local compressed_gain = 10.0 ^ (-reduction_db / 20.0) * makeup
        local wet_gain = (1.0 - mix) + mix * compressed_gain

        reduction_sum = reduction_sum + reduction_db

        for channel = 0, channels - 1 do
            local input = audio.get(channel, sample) or 0.0
            audio.set(channel, sample, input * wet_gain)
        end
    end

    host.trace(reduction_sum / math.max(1, num_samples) / 24.0)
end

function draw()
    ui.heading("Stereo Compressor")
    ui.label("Linked peak detector with external sidechain blend")

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
                local ratio = 1.0 + params.get(2) * 9.0
                local knee = params.get(8)
                for i = 0, 128 do
                    local input = i / 128.0
                    local input_db = -60.0 + input * 60.0
                    local over = math.max(0.0, input_db - threshold)
                    local knee_width = math.max(0.001, knee)
                    local soft = over
                    if over < knee_width then
                        soft = over * over / (2.0 * knee_width)
                    else
                        soft = over - knee_width / 2.0
                    end
                    local reduction = soft * (1.0 - 1.0 / math.max(1.0, ratio))
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
