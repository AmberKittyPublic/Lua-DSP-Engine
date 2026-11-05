local ui_module = {}

local ATTACK_SCALE = { type = "float", min = 0.001, max = 0.1 }
local DECAY_SCALE = { type = "float", min = 0.01, max = 1.0 }
local SUSTAIN_SCALE = { type = "float", min = 0.0, max = 1.0 }
local RELEASE_SCALE = { type = "float", min = 0.01, max = 1.0 }
local BUFFER_SCALE = { type = "float", min = 0.0, max = 100.0 }
local DISTORTION_MODES = {
    "Saturator",
    "Bitcrush",
    "Tube",
    "Tape",
    "Overdrive",
    "Fuzz",
    "Diode",
}
local DISTORTION_DRIVE_SCALE = { type = "float", min = 0.0, max = 36.0 }
local DISTORTION_ENV_SCALE = { type = "float", min = 0.0, max = 24.0 }
local DISTORTION_MIX_SCALE = { type = "float", min = 0.0, max = 1.0 }
local DISTORTION_CHARACTER_SCALE = { type = "float", min = 0.0, max = 1.0 }
local DISTORTION_BIAS_SCALE = { type = "float", min = -1.0, max = 1.0 }
local DISTORTION_BITS_SCALE = { type = "int", min = 1, max = 16 }
local DISTORTION_DOWNSAMPLE_SCALE = { type = "int", min = 1, max = 32 }

local function clamp(value, low, high)
    return math.max(low, math.min(high, value))
end

local function shape_sample(model, value, bias, character, bits)
    if model == 0 then
        return math.tanh((value + bias * 0.15) * (1.0 + character * 4.0))
    elseif model == 1 then
        local levels = 2.0 ^ (bits - 1)
        return math.floor(clamp(value, -1.0, 1.0) * levels + 0.5) / levels
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
    local excess = math.abs(diode) - (0.05 + character * 0.35)
    if excess <= 0.0 then
        return 0.0
    end
    local polarity = diode < 0.0 and -1.0 or 1.0
    return polarity * (1.0 - math.exp(-excess * (3.0 + character * 15.0)))
end

local function transfer_value(input, drive, model, bias, character, bits, mix)
    local wet = shape_sample(model, input * drive, bias, character, bits)
    return input * (1.0 - mix) + wet * mix
end

local function draw_transfer_graph(model, base_drive, peak_drive, bias, character, bits, mix)
    local clean = {}
    local current = {}
    local envelope_peak = {}
    for index = 0, 128 do
        local input = -1.0 + 2.0 * index / 128.0
        clean[index + 1] = (input + 1.0) * 0.5
        current[index + 1] = (clamp(transfer_value(
            input, base_drive, model, bias, character, bits, mix
        ), -1.0, 1.0) + 1.0) * 0.5
        envelope_peak[index + 1] = (clamp(transfer_value(
            input, peak_drive, model, bias, character, bits, mix
        ), -1.0, 1.0) + 1.0) * 0.5
    end
    ui.plot(clean, "gray")
    ui.plot(current, "orange")
    ui.plot(envelope_peak, "#61d6c3")
end

local function draw_cycle_graph(model, base_drive, peak_drive, bias, character, bits, mix, downsample)
    local input_curve = {}
    local current = {}
    local envelope_peak = {}
    local current_hold = 0.0
    local peak_hold = 0.0
    for index = 0, 127 do
        local input = math.sin(index / 128.0 * math.pi * 2.0)
        input_curve[index + 1] = (input + 1.0) * 0.5

        local current_wet = shape_sample(model, input * base_drive, bias, character, bits)
        local peak_wet = shape_sample(model, input * peak_drive, bias, character, bits)
        if model == 1 then
            if index % downsample == 0 then
                current_hold = current_wet
                peak_hold = peak_wet
            end
            current_wet = current_hold
            peak_wet = peak_hold
        end
        current[index + 1] = (clamp(input * (1.0 - mix) + current_wet * mix, -1.0, 1.0) + 1.0) * 0.5
        envelope_peak[index + 1] = (clamp(input * (1.0 - mix) + peak_wet * mix, -1.0, 1.0) + 1.0) * 0.5
    end
    ui.plot(input_curve, "gray")
    ui.plot(current, "orange")
    ui.plot(envelope_peak, "#61d6c3")
end

function ui_module.draw(params)
    local model = math.floor(params.get(15) + 0.5)
    local drive_db = params.get(16)
    local envelope_depth_db = params.get(17)
    local mix = params.get(18)
    local character = params.get(19)
    local bias = params.get(20)
    local bits = params.get(21)
    local downsample = math.max(1, math.floor(params.get(22) + 0.5))
    local base_drive = 10.0 ^ (drive_db / 20.0)
    local peak_drive = 10.0 ^ ((drive_db + envelope_depth_db) / 20.0)
    local live_waveform = host.waveform_trace()
    local waveform_curve = {}
    local waveform_center = {}
    if #live_waveform == 0 then
        for index = 1, 128 do
            waveform_curve[index] = 0.5
            waveform_center[index] = 0.5
        end
    else
        for index, sample in ipairs(live_waveform) do
            waveform_curve[index] = (clamp(sample, -1.0, 1.0) + 1.0) * 0.5
            waveform_center[index] = 0.5
        end
    end

    ui.colored_heading("ACID 303", "#ffad55")
    ui.label("MONO ACID SYNTH  /  RESONANT FILTER  /  ENVELOPE-DRIVEN DISTORTION")
    ui.separator_horizontal()

    ui.row(function()
        ui.column(function()
            ui.group("VOICE", function()
                ui.row(function()
                    ui.knob(1, "Level")
                    ui.knob(2, "Tune")
                    ui.toggle(7, "Square wave")
                end)
            end)

            ui.group("FILTER & SEQUENCE", function()
                ui.row(function()
                    ui.knob(3, "Cutoff")
                    ui.knob(4, "Resonance")
                    ui.knob(5, "Env mod")
                end)
                ui.row(function()
                    ui.knob(8, "Accent")
                    ui.knob(9, "Slide")
                    ui.knob(14, "Buffer", BUFFER_SCALE)
                end)
            end)

            ui.group("AMPLITUDE ENVELOPE", function()
                ui.row(function()
                    ui.knob(10, "Attack", ATTACK_SCALE)
                    ui.knob(11, "Decay", DECAY_SCALE)
                    ui.knob(12, "Sustain", SUSTAIN_SCALE)
                    ui.knob(13, "Release", RELEASE_SCALE)
                end)
            end)
        end)

        ui.column(function()
            ui.group("DISTORTION SUITE", function()
                ui.radio_param(15, "Model", DISTORTION_MODES)
                ui.row(function()
                    ui.knob(16, "Drive", DISTORTION_DRIVE_SCALE)
                    ui.knob(17, "Env drive", DISTORTION_ENV_SCALE)
                    ui.knob(18, "Mix", DISTORTION_MIX_SCALE)
                end)
                ui.row(function()
                    ui.knob(19, "Character", DISTORTION_CHARACTER_SCALE)
                    ui.knob(20, "Bias", DISTORTION_BIAS_SCALE)
                end)
                ui.row(function()
                    ui.knob(21, "Bit depth", DISTORTION_BITS_SCALE)
                    ui.knob(22, "Downsample", DISTORTION_DOWNSAMPLE_SCALE)
                end)
                ui.label(string.format("ADSR drive range: %.1f dB to %.1f dB", drive_db, drive_db + envelope_depth_db))
            end)
        end)
    end)

    ui.separator_horizontal()
    ui.colored_heading("SOUND SHAPE", "#61d6c3")
    ui.label("Gray: clean input   Orange: current drive   Teal: envelope-peak drive")
    ui.row(function()
        ui.column(function()
            ui.label("TRANSFER CURVE")
            ui.graph(function()
                draw_transfer_graph(model, base_drive, peak_drive, bias, character, bits, mix)
            end)
        end)
        ui.column(function()
            ui.label("ONE-CYCLE SINE PREVIEW")
            ui.graph(function()
                draw_cycle_graph(
                    model, base_drive, peak_drive, bias, character, bits, mix, downsample
                )
            end)
        end)
    end)

    ui.group("LIVE OUTPUT WAVEFORM", function()
        ui.row(function()
            ui.column(function()
                ui.graph(function()
                    ui.plot(waveform_center, "gray")
                    ui.plot(waveform_curve, "#61d6c3")
                end)
            end)
            ui.column(function()
                ui.label("CHANNEL 1 / POST DISTORTION")
                ui.label("Teal: recent audio   Gray: zero line")
                if #live_waveform == 0 then
                    ui.label("Waiting for audio output. Play a note to start the scope.")
                else
                    ui.label(string.format("Rolling window: %d samples", #live_waveform))
                end
                ui.meter(host.meter(), "Output peak")
            end)
        end)
    end)
end

return ui_module
