-- MIDI-triggered sine kick with a low body, pitch drop, and fixed-frequency click.
-- Macro 1: level, Macro 2: tuning, Macro 3: pitch drop, Macro 4: pitch time,
-- Macro 5: body decay, Macro 6: click level.

local voices = {}

params.set_defaults({
    [1] = 0.65,
    [2] = 0.5,
    [3] = 0.5,
    [4] = 0.25,
    [5] = 0.3,
    [6] = 0.25,
})

local function configure_voice(state)
    state.base_frequency = 48.0 * 2.0 ^ ((params.get(2) - 0.5) * 2.0)
    state.body:set_pitch_drop(12.0 + params.get(3) * 24.0, 0.008 + params.get(4) * 0.06)
end

function process_voice(voice)
    local state = voices[voice.id]
    if not state then
        local body = synth.osc("sine")
        local click = synth.osc("sine")
        click:set_lowpass(9000, 0.05)

        state = {
            body = body,
            click = click,
            body_env = synth.adsr(0.0005, 0.06 + params.get(5) * 0.22, 0.0, 0.01),
            click_env = synth.adsr(0.0001, 0.006, 0.0, 0.002),
            age = voice.age,
        }
        configure_voice(state)
        state.body_env:trigger()
        state.click_env:trigger()
        voices[voice.id] = state
    elseif state.age ~= voice.age then
        state.age = voice.age
        configure_voice(state)
        state.body_env:trigger()
        state.click_env:trigger()
    end

    local level = 0.08 + params.get(1) * 0.32
    state.body:render_env(state.base_frequency, host.sample_rate, voice.velocity * level, state.body_env)
    state.click:render_env(
        2600.0,
        host.sample_rate,
        voice.velocity * params.get(6) * 0.18,
        state.click_env
    )

    if state.body_env:value() < 0.0001 and state.click_env:value() < 0.0001 then
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
    ui.label("Low sine body with a sample-smooth pitch drop and short click")
    ui.param(1, "Level")
    ui.param(2, "Tune")
    ui.param(3, "Pitch drop")
    ui.param(4, "Pitch time")
    ui.param(5, "Body decay")
    ui.param(6, "Click")
    ui.separator()
    ui.label("Trigger with MIDI notes; note length does not cut off the one-shot.")
end
