-- MIDI-triggered sine kick with a low body, pitch drop, and filtered white-noise layer.
-- Macro 1: level, Macro 2: tuning, Macro 3: pitch drop, Macro 4: pitch time,
-- Macro 5: body decay, Macro 6: noise level, Macro 7: FM harmonics.

local voices = {}

params.set_defaults({
    [1] = 0.65,
    [2] = 0.5,
    [3] = 0.35,
    [4] = 0.2,
    [5] = 0.3,
    [6] = 0.5,
    [7] = 0.5,
})

local function configure_voice(state)
    state.base_frequency = 48.0 * 2.0 ^ ((params.get(2) - 0.5) * 2.0)
    state.body:set_pitch_drop(6.0 + params.get(3) * 18.0, 0.008 + params.get(4) * 0.06)
end

function process_voice(voice)
    local state = voices[voice.id]
    if not state then
        local body = synth.osc("sine")
        local noise = synth.osc("white")
        noise:set_lowpass(8500, 0.05)
        noise:set_lowpass_2(1800, 0.12)

        state = {
            body = body,
            noise = noise,
            body_env = synth.adsr(0.0005, 0.06 + params.get(5) * 0.22, 0.0, 0.01),
            noise_env = synth.adsr(0.0005, 0.06 + params.get(5) * 0.22, 0.0, 0.01),
            age = voice.age,
        }
        configure_voice(state)
        state.body_env:trigger()
        state.noise_env:trigger()
        voices[voice.id] = state
    elseif state.age ~= voice.age then
        state.age = voice.age
        configure_voice(state)
        state.body_env:trigger()
        state.noise_env:trigger()
    end

    local body_decay = 0.06 + params.get(5) * 0.22
    state.body_env:set_adsr(0.0005, body_decay, 0.0, 0.01)
    state.noise_env:set_adsr(0.0005, body_decay, 0.0, 0.01)

    local level = 0.08 + params.get(1) * 0.32
    state.body:set_fm(2.0, 0.3 + params.get(7) * 2.2)
    state.body:render_env(state.base_frequency, host.sample_rate, voice.velocity * level, state.body_env)
    state.noise:render_filtered(
        state.base_frequency,
        host.sample_rate,
        voice.velocity * params.get(6) * 0.3,
        state.noise_env,
        nil,
        2.4,
        0.0
    )

    if state.body_env:value() < 0.0001 and state.noise_env:value() < 0.0001 then
        voices[voice.id] = nil
        return true
    end
    return false
end

function voice_ended(id)
    voices[id] = nil
end

function draw()
    ui.heading("Kick Drum")
    ui.label("Low sine/FM body plus white noise shaped by the same ADSR")
    ui.param(1, "Level")
    ui.param(2, "Tune")
    ui.param(3, "Pitch drop")
    ui.param(4, "Pitch time")
    ui.param(5, "Body decay")
    ui.param(6, "Noise layer")
    ui.param(7, "FM harmonics")
    ui.separator()
    ui.label("Trigger with MIDI notes; note length does not cut off the one-shot.")
end
