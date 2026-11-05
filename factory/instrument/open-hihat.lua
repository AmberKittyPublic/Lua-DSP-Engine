-- Open hi-hat from inharmonic square partials plus a small noise layer.
-- Macro 1: level, Macro 2: brightness, Macro 3: decay, Macro 4: noise amount.

local voices = {}
local partial_frequencies = { 205.3, 304.4, 369.6, 522.7, 800.0, 1030.0 }

params.set_defaults({
    [1] = 0.65,
    [2] = 0.5,
    [3] = 0.55,
    [4] = 0.65,
})

function process_voice(voice)
    local state = voices[voice.id]
    if not state then
        local decay = 0.1 + params.get(3) * 0.55
        local tones = {}
        for _, frequency in ipairs(partial_frequencies) do
            local oscillator = synth.osc("square")
            oscillator:set_unison(2, 3, 1.0)
            tones[#tones + 1] = {
                osc = oscillator,
                frequency = frequency,
                env = synth.adsr(0.001, decay, 0.0, 0.01),
            }
        end

        local noise = synth.osc("white")
        state = {
            tones = tones,
            noise = noise,
            noise_env = synth.adsr(0.001, math.min(decay, 0.06), 0.0, 0.008),
            age = voice.age,
        }
        for _, tone in ipairs(state.tones) do
            tone.env:trigger()
        end
        state.noise_env:trigger()
        voices[voice.id] = state
    elseif state.age ~= voice.age then
        state.age = voice.age
        for _, tone in ipairs(state.tones) do
            tone.env:trigger()
        end
        state.noise_env:trigger()
    end

    local decay = 0.1 + params.get(3) * 0.55
    local cutoff = 3500.0 + params.get(2) * 4500.0
    for _, tone in ipairs(state.tones) do
        tone.env:set_adsr(0.001, decay, 0.0, 0.01)
        tone.osc:set_highpass(cutoff, 0.15)
        tone.osc:set_lowpass(16000.0, 0.08)
    end
    state.noise_env:set_adsr(0.001, math.min(decay, 0.06), 0.0, 0.008)
    state.noise:set_unison(2, 0, 0.8)
    state.noise:set_highpass(cutoff, 0.12)
    state.noise:set_lowpass(16000.0, 0.08)
    local gain = voice.velocity * (0.04 + params.get(1) * 0.2)
    for _, tone in ipairs(state.tones) do
        tone.osc:render_env(tone.frequency, host.sample_rate, gain * 0.12, tone.env)
    end
    state.noise:render_env(8000.0, host.sample_rate, gain * params.get(4) * 0.16, state.noise_env)

    local done = state.noise_env:value() < 0.0001
    for _, tone in ipairs(state.tones) do
        done = done and tone.env:value() < 0.0001
    end
    if done then
        voices[voice.id] = nil
        return true
    end
    return false
end

function voice_ended(id)
    voices[id] = nil
end

function draw()
    ui.heading("Open Hi-Hat")
    ui.label("Inharmonic square partials with a filtered noise layer")
    ui.param(1, "Level")
    ui.param(2, "Brightness")
    ui.param(3, "Decay")
    ui.param(4, "Noise")
end
