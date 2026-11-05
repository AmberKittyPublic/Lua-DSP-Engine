-- Soft-clipping saturator.
-- Macro 1: drive, Macro 2: tone, Macro 3: output, Macro 4: mix,
-- Macro 5: bias, Macro 6: low cut, Macro 7: shape.

local drive_scale = { type = "float", min = 0.0, max = 36.0 }
local tone_scale = { type = "float", min = 0.0, max = 1.0 }
local output_scale = { type = "float", min = -12.0, max = 12.0 }
local mix_scale = { type = "float", min = 0.0, max = 1.0 }
local bias_scale = { type = "float", min = -1.0, max = 1.0 }
local low_cut_scale = { type = "float", min = 20.0, max = 500.0 }
local shape_scale = { type = "float", min = 0.0, max = 1.0 }

params.set_range(1, drive_scale)
params.set_range(2, tone_scale)
params.set_range(3, output_scale)
params.set_range(4, mix_scale)
params.set_range(5, bias_scale)
params.set_range(6, low_cut_scale)
params.set_range(7, shape_scale)

params.set_defaults({
    [1] = 6.0,
    [2] = 1.0,
    [3] = 0.0,
    [4] = 0.5,
    [5] = 0.0,
    [6] = 20.0,
    [7] = 0.35,
})

local lp_state = {}
local hp_state = {}
function process(num_samples, sample_rate, tempo, playing, pos_beats)
    if sample_rate <= 0.0 or num_samples <= 0 then
        return
    end

    local channels = audio.num_channels()
    local drive_db = params.get(1)
    local tone = params.get(2)
    local output_db = params.get(3)
    local mix = params.get(4)
    local bias = params.get(5)
    local low_cut = params.get(6)
    local shape = params.get(7)

    local drive = 10.0 ^ (drive_db / 20.0)
    local output = 10.0 ^ (output_db / 20.0)

    local cutoff = 1000.0 * (2.0 ^ (tone * 4.3219))
    if cutoff > sample_rate * 0.45 then
        cutoff = sample_rate * 0.45
    end
    local lp_alpha = 1.0 - math.exp(-2.0 * math.pi * cutoff / sample_rate)
    if tone >= 0.999 then
        lp_alpha = 1.0
    end

    local hp_alpha = math.exp(-2.0 * math.pi * low_cut / sample_rate)

    for channel = 0, channels - 1 do
        if lp_state[channel] == nil then
            lp_state[channel] = 0.0
        end
        if hp_state[channel] == nil then
            hp_state[channel] = 0.0
        end
    end

    for sample = 0, num_samples - 1 do
        for channel = 0, channels - 1 do
            local input = audio.get(channel, sample) or 0.0

            local highpassed = input - hp_state[channel]
            hp_state[channel] = hp_state[channel] * hp_alpha + input * (1.0 - hp_alpha)
            -- one-pole highpass approximation

            local driven = (highpassed + bias * 0.15) * drive
            local saturation = 1.0 + shape * 4.0
            local saturated = math.tanh(driven * saturation)
            local shaped = saturated * output

            local previous = lp_state[channel] or 0.0
            local filtered = previous + lp_alpha * (shaped - previous)
            lp_state[channel] = filtered

            local result = input * (1.0 - mix) + filtered * mix
            if result > 1.0 then
                result = 1.0
            elseif result < -1.0 then
                result = -1.0
            end
            audio.set(channel, sample, result)
        end
    end
end

function draw()
    ui.colored_heading("Saturator", "red")
    ui.label("Soft-clipping saturator with tone, bias, and filtering")

    ui.row(function()
        ui.column(function()
            ui.group("Gain", function()
                ui.row(function()
                    ui.knob(1, "Drive", drive_scale)
                    ui.knob(3, "Output", output_scale)
                    ui.knob(4, "Mix", mix_scale)
                end)
            end)

            ui.group("Tone", function()
                ui.row(function()
                    ui.knob(2, "Tone", tone_scale)
                    ui.knob(6, "Low cut", low_cut_scale)
                    ui.knob(7, "Shape", shape_scale)
                    ui.knob(5, "Bias", bias_scale)
                end)
            end)
        end)

        ui.graph(function()
            local drive = 10.0 ^ (params.get(1) / 20.0)
            local bias = params.get(5)
            local shape = params.get(7)
            local output = 10.0 ^ (params.get(3) / 20.0)
            local curve = {}
            for index = 0, 128 do
                local input = -1.0 + 2.0 * index / 128.0
                local driven = (input + bias * 0.15) * drive
                local saturated = math.tanh(driven * (1.0 + shape * 4.0))
                local value = math.max(-1.0, math.min(1.0, saturated * output))
                curve[index + 1] = (value + 1.0) * 0.5
            end
            ui.plot(curve, "red")
        end)
    end)

    ui.separator_horizontal()
    ui.meter(host.meter(), "output peak")
end
