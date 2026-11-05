-- Stereo feedback delay.
-- Macro 1: delay time (50-700 ms), Macro 2: feedback, Macro 3: wet mix,
-- Macro 4: ping-pong feedback.

params.set_defaults({
    [1] = 0.25,
    [2] = 0.25,
    [3] = 0.4,
    [4] = 0.0,
})

local max_delay_seconds = 2.0
local buffer_size = math.max(2, math.floor(host.sample_rate * max_delay_seconds))
local left = {}
local right = {}
for index = 1, buffer_size do
    left[index] = 0.0
    right[index] = 0.0
end

local write_index = 1
local time_lfo = synth.lfo("sine", 0.17)

function process(num_samples, sample_rate, tempo, playing, pos_beats)
    local wobble = time_lfo:process(num_samples, sample_rate) * 0.003
    local delay_seconds = math.max(0.05, math.min(0.7, 0.05 + params.get(1) * 0.65 + wobble))
    local delay_samples = math.min(
        buffer_size - 1,
        math.max(1, math.floor(delay_seconds * sample_rate))
    )
    local feedback = 0.2 + params.get(2) * 0.68
    local wet = 0.25 + params.get(3) * 0.6
    local ping_pong = params.get(4) >= 0.5
    local channels = audio.num_channels()

    for sample = 0, num_samples - 1 do
        local read_index = write_index - delay_samples
        if read_index <= 0 then
            read_index = read_index + buffer_size
        end

        local input_left = audio.get(0, sample) or 0.0
        local input_right = audio.get(1, sample) or input_left
        local delayed_left = left[read_index]
        local delayed_right = right[read_index]

        audio.set(0, sample, input_left * (1.0 - wet) + delayed_left * wet)
        if channels > 1 then
            audio.set(1, sample, input_right * (1.0 - wet) + delayed_right * wet)
        end

        if ping_pong then
            left[write_index] = input_left + delayed_right * feedback
            right[write_index] = input_right + delayed_left * feedback
        else
            left[write_index] = input_left + delayed_left * feedback
            right[write_index] = input_right + delayed_right * feedback
        end

        write_index = write_index + 1
        if write_index > buffer_size then
            write_index = 1
        end
    end
end

function draw()
    ui.heading("Stereo Delay")
    ui.label("Feedback delay with optional ping-pong routing")
    ui.label("Subtle Rust-side LFO modulation on delay time")
    ui.param(1, "Time")
    ui.param(2, "Feedback")
    ui.param(3, "Wet")
    ui.param(4, "Ping-pong")
    ui.separator()
    ui.meter(host.meter(), "output peak")
end
