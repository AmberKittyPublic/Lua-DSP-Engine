-- Snare with a tuned sine body and filtered white-noise wires.
-- Macro 1: level, Macro 2: tuning, Macro 3: noise tone,
-- Macro 4: decay, Macro 5: FM body, Macro 6: noise level.

local voices = {}

params.set_defaults({
    [1] = 0.65,
    [2] = 0.5,
    [3] = 0.45,
    [4] = 0.35,
    [5] = 0.2,
    [6] = 0.7,
})

local function configure_body(state)
    state.frequency = 180.0 * 2.0 ^ ((params.get(2) - 0.5) * 2.0)
    state.body:set_pitch_drop(3.0 + params.get(2) * 6.0, 0.012)
end

function process_voice(voice)
    local state = voices[voice.id]
    if not state then
        local body = synth.osc("sine")
        local wires = synth.osc("pink")
        wires:set_highpass(1000.0, 0.15)
        wires:set_lowpass(12000.0, 0.08)
        state = {
            body = body,
            wires = wires,
            body_env = synth.adsr(0.001, 0.06 + params.get(4) * 0.18, 0.0, 0.01),
            wire_env = synth.adsr(0.001, 0.08 + params.get(4) * 0.25, 0.0, 0.015),
            age = voice.age,
        }
        configure_body(state)
        state.body_env:trigger()
        state.wire_env:trigger()
        voices[voice.id] = state
    elseif state.age ~= voice.age then
        state.age = voice.age
        configure_body(state)
        state.body_env:trigger()
        state.wire_env:trigger()
    end

    local body_decay = 0.06 + params.get(4) * 0.18
    local wire_decay = 0.08 + params.get(4) * 0.25
    state.body_env:set_adsr(0.001, body_decay, 0.0, 0.01)
    state.wire_env:set_adsr(0.001, wire_decay, 0.0, 0.015)
    state.wires:set_highpass(700.0 + params.get(3) * 4500.0, 0.15)
    state.body:set_fm(2.0, params.get(5) * 2.0)

    local level = voice.velocity * (0.04 + params.get(1) * 0.2)
    state.body:render_env(state.frequency, host.sample_rate, level * 0.75, state.body_env)
    state.wires:render_filtered(
        state.frequency,
        host.sample_rate,
        level * params.get(6) * 0.5,
        state.wire_env,
        nil,
        1.5,
        0.0
    )

    if state.body_env:value() < 0.0001 and state.wire_env:value() < 0.0001 then
        voices[voice.id] = nil
        return true
    end
    return false
end

function voice_ended(id)
    voices[id] = nil
end

function draw()
    ui.heading("Snare")
    ui.label("Tuned FM body layered with filtered pink-noise wires")
    ui.param(1, "Level")
    ui.param(2, "Tune")
    ui.param(3, "Wire tone")
    ui.param(4, "Decay")
    ui.param(5, "FM body")
    ui.param(6, "Wire level")
end
