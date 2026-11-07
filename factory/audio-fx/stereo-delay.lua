-- Stereo delay with tempo sync, distinct feedback paths, and character modes.
-- Macros 1-4 retain time, feedback, wet, and ping-pong. Macros 5-13 add
-- character, sync, division, tone, modulation, width, drive, bits, and wear.

local modes = {
    { name = "Digital", description = "Clean, full-band repeats", color = "#70d6c7", tone = 1.0, modulation = 0.0, feedback = 1.0 },
    { name = "Tape", description = "Warm saturation with gentle wow", color = "#f0ad72", tone = 0.62, modulation = 1.0, feedback = 0.96 },
    { name = "Analog", description = "Darkened repeats with soft edges", color = "#91a9ff", tone = 0.48, modulation = 0.4, feedback = 0.94 },
    { name = "BBD", description = "Band-limited bucket-brigade color", color = "#d49bff", tone = 0.4, modulation = 0.24, feedback = 0.9 },
    { name = "Lo-fi", description = "Reduced resolution and worn repeats", color = "#f2768b", tone = 0.3, modulation = 0.0, feedback = 0.84 },
}
local mode_names = { "Digital", "Tape", "Analog", "BBD", "Lo-fi" }
local divisions = { "1/4", "1/8", "1/8 dotted", "1/8 triplet", "1/16", "1/16 dotted", "1/16 triplet", "1/32" }
local division_beats = { 1.0, 0.5, 0.75, 1.0 / 3.0, 0.25, 0.375, 1.0 / 6.0, 0.125 }

local mode_scale = { type = "int", min = 0, max = 4 }
local toggle_scale = { type = "int", min = 0, max = 1 }
local division_scale = { type = "int", min = 0, max = 7 }
local float_scale = { type = "float", min = 0.0, max = 1.0 }
local bits_scale = { type = "int", min = 4, max = 16 }
local wear_scale = { type = "int", min = 1, max = 8 }

params.set_range(5, mode_scale)
params.set_range(6, toggle_scale)
params.set_range(7, division_scale)
params.set_range(8, float_scale)
params.set_range(9, float_scale)
params.set_range(10, float_scale)
params.set_range(11, float_scale)
params.set_range(12, bits_scale)
params.set_range(13, wear_scale)

params.set_defaults({
    [1] = 0.25,
    [2] = 0.25,
    [3] = 0.4,
    [4] = 0.0,
    [5] = 0,
    [6] = 0,
    [7] = 4,
    [8] = 1.0,
    [9] = 0.1,
    [10] = 1.0,
    [11] = 0.25,
    [12] = 10,
    [13] = 2,
})

local max_delay_seconds = 2.0
local buffer_size = math.max(2, math.floor(host.sample_rate * max_delay_seconds))
local left, right = {}, {}
for index = 1, buffer_size do
    left[index] = 0.0
    right[index] = 0.0
end

local write_index = 1
local filter_left, filter_right = 0.0, 0.0
local modulation_phase = 0.0
local held_left, held_right = 0.0, 0.0
local wear_counter = 0

local function read_interpolated(buffer, position)
    position = (position - 1.0) % buffer_size + 1.0
    local lower = math.floor(position)
    local upper = lower + 1
    if upper > buffer_size then
        upper = 1
    end
    local fraction = position - lower
    return buffer[lower] * (1.0 - fraction) + buffer[upper] * fraction
end

local function shape_feedback(value, character, drive, bits)
    if character == 1 then
        return math.tanh(value * drive) / drive
    elseif character == 2 then
        return math.tanh(value * (1.0 + drive * 0.45)) / (1.0 + drive * 0.45)
    elseif character == 3 then
        local bbd_drive = 1.2 + (drive - 1.0) * 0.15
        local compressed = math.tanh(value * bbd_drive) / bbd_drive
        local levels = 2.0 ^ (bits - 1)
        return math.floor(compressed * levels + 0.5) / levels
    elseif character == 4 then
        local levels = 2.0 ^ (bits - 1)
        return math.floor(value * levels + 0.5) / levels
    end
    return value
end

function process(num_samples, sample_rate, tempo, playing, pos_beats)
    if sample_rate <= 0.0 or num_samples <= 0 then
        return
    end

    local character = math.max(0, math.min(4, math.floor(params.get(5) + 0.5)))
    local mode = modes[character + 1]
    local synced = params.get(6) >= 0.5 and tempo > 0.0
    local division = math.max(0, math.min(7, math.floor(params.get(7) + 0.5)))
    local delay_seconds = synced
        and (60.0 / tempo) * division_beats[division + 1]
        or (0.05 + params.get(1) * 0.65)
    local delay_samples = math.max(1.0, math.min(buffer_size - 2.0, delay_seconds * sample_rate))
    local feedback = (0.2 + params.get(2) * 0.68) * mode.feedback
    local wet = 0.25 + params.get(3) * 0.6
    local ping_pong = params.get(4) >= 0.5
    local channels = audio.num_channels()
    if channels <= 0 then
        return
    end
    local tone = math.min(params.get(8), mode.tone)
    local cutoff = 700.0 + tone * tone * 15000.0
    local tone_alpha = character == 0 and 1.0
        or 1.0 - math.exp(-2.0 * math.pi * math.min(cutoff, sample_rate * 0.45) / sample_rate)
    local drive = 1.0 + params.get(11) * (character == 1 and 7.0 or 3.0)
    local tape_drive = 1.0 + params.get(11) * 2.0
    local bits = params.get(12)
    local wear = params.get(13)
    local modulation_depth = params.get(9) * mode.modulation * 0.004 * sample_rate
    local width = params.get(10)
    local phase_step = 2.0 * math.pi * (character == 1 and 0.21 or 0.13) / sample_rate

    for sample = 0, num_samples - 1 do
        local modulation = math.sin(modulation_phase) * modulation_depth
        modulation_phase = modulation_phase + phase_step
        if modulation_phase >= 2.0 * math.pi then
            modulation_phase = modulation_phase - 2.0 * math.pi
        end

        local input_left = audio.get(0, sample) or 0.0
        local input_right = channels > 1 and (audio.get(1, sample) or input_left) or input_left
        local delayed_left = read_interpolated(left, write_index - delay_samples + modulation)
        local delayed_right = read_interpolated(right, write_index - delay_samples - modulation)

        filter_left = filter_left + tone_alpha * (delayed_left - filter_left)
        filter_right = filter_right + tone_alpha * (delayed_right - filter_right)
        local feedback_left = filter_left
        local feedback_right = filter_right
        if character == 3 then
            feedback_left = feedback_left * 0.92
            feedback_right = feedback_right * 0.92
        elseif character == 4 then
            feedback_left = feedback_left * 0.8
            feedback_right = feedback_right * 0.8
        end

        local wet_left = delayed_left
        local wet_right = delayed_right
        if character == 1 then
            wet_left = math.tanh(wet_left * tape_drive) / tape_drive
            wet_right = math.tanh(wet_right * tape_drive) / tape_drive
        elseif character == 2 then
            wet_left = math.tanh(wet_left * 1.35) / 1.35
            wet_right = math.tanh(wet_right * 1.35) / 1.35
        end

        if channels > 1 then
            local mid = (wet_left + wet_right) * 0.5
            local side = (wet_left - wet_right) * 0.5 * width
            wet_left = mid + side
            wet_right = mid - side
        end
        local output_left = input_left * (1.0 - wet) + wet_left * wet
        local output_right = input_right * (1.0 - wet) + wet_right * wet
        audio.set(0, sample, output_left)
        if channels > 1 then
            audio.set(1, sample, output_right)
        end

        local next_left, next_right
        if ping_pong then
            next_left = input_left + feedback_right * feedback
            next_right = input_right + feedback_left * feedback
        else
            next_left = input_left + feedback_left * feedback
            next_right = input_right + feedback_right * feedback
        end

        if character == 4 and wear_counter ~= 0 then
            next_left = held_left
            next_right = held_right
        else
            next_left = shape_feedback(next_left, character, drive, bits)
            next_right = shape_feedback(next_right, character, drive, bits)
            held_left, held_right = next_left, next_right
        end
        wear_counter = (wear_counter + 1) % wear
        left[write_index] = next_left
        right[write_index] = next_right

        write_index = write_index + 1
        if write_index > buffer_size then
            write_index = 1
        end
    end
end

function draw()
    local character = math.max(0, math.min(4, math.floor(params.get(5) + 0.5)))
    local mode = modes[character + 1]
    local synced = params.get(6) >= 0.5
    local free_ms = (0.05 + params.get(1) * 0.65) * 1000.0
    local division = math.max(0, math.min(7, math.floor(params.get(7) + 0.5)))
    local feedback = (0.2 + params.get(2) * 0.68) * mode.feedback
    local ping_pong = params.get(4) >= 0.5
    local left_echo, right_echo = {}, {}

    for tap = 0, 31 do
        local amplitude = feedback ^ tap
        local goes_left = not ping_pong or tap % 2 == 0
        left_echo[tap + 1] = 0.5 + (goes_left and amplitude or 0.0) * 0.48
        right_echo[tap + 1] = 0.5 - (goes_left and 0.0 or amplitude) * 0.48
    end

    ui.colored_heading("STEREO DELAY / " .. string.upper(mode.name), mode.color)
    ui.label(mode.description .. " | tempo sync, motion, and tone")
    ui.radio_param(5, "Character", mode_names)

    ui.row(function()
        ui.column(function()
            ui.group("Timing and routing", function()
                ui.knob(1, "Free time")
                ui.knob(2, "Feedback")
                ui.knob(3, "Wet")
                ui.toggle(4, "Ping-pong")
                ui.toggle(6, "Tempo sync")
                if synced then
                    ui.radio_param(7, "Division", divisions)
                end
            end)
            ui.group("Echo color", function()
                ui.knob(10, "Stereo width", float_scale)
                if character > 0 then
                    ui.knob(8, "Feedback tone", float_scale)
                end
                if mode.modulation > 0.0 then
                    ui.knob(9, "Modulation", float_scale)
                end
                if character == 1 or character == 2 or character == 3 then
                    ui.knob(11, "Drive", float_scale)
                end
                if character >= 3 then
                    ui.knob(12, "Bit depth", bits_scale)
                end
                if character == 4 then
                    ui.knob(13, "Wear / hold", wear_scale)
                end
            end)
        end)
        ui.column(function()
            ui.group("Echo pattern", function()
                ui.graph(function()
                    ui.plot(left_echo, "#61d6c3")
                    ui.plot(right_echo, "#f0ad72")
                end)
                ui.colored_heading("LEFT REPEATS", "#61d6c3")
                ui.colored_heading("RIGHT REPEATS", "#f0ad72")
                if synced then
                    ui.label("Synced division: " .. divisions[division + 1])
                else
                    ui.label(string.format("Free delay: %.0f ms", free_ms))
                end
            end)
            ui.group("Signal", function()
                ui.colored_meter(math.min(1.0, host.input_meter()), "Input", "#61d6c3")
                ui.colored_meter(math.min(1.0, host.meter()), "Output", mode.color)
            end)
        end)
    end)
end
