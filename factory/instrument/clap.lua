-- Wide layered clap built from pink and white noise.
-- Macro 1: level, Macro 2: brightness, Macro 3: tail, Macro 4: stereo width.

local voices = {}

params.set_defaults({
    [1] = 0.7,
    [2] = 0.45,
    [3] = 0.35,
    [4] = 0.7,
})

function process_voice(voice)
    local state = voices[voice.id]
    if not state then
        local body = synth.osc("pink")
        local bright = synth.osc("white")
        body:set_unison(2, 0, 0.7)
        bright:set_unison(2, 0, 1.0)
        state = {
            body = body,
            bright = bright,
            body_env = synth.adsr(0.001, 0.04 + params.get(3) * 0.16, 0.0, 0.015),
            bright_env = synth.adsr(0.001, 0.01, 0.0, 0.005),
            age = voice.age,
        }
        state.body_env:trigger()
        state.bright_env:trigger()
        voices[voice.id] = state
    elseif state.age ~= voice.age then
        state.age = voice.age
        state.body_env:trigger()
        state.bright_env:trigger()
    end

    local brightness = params.get(2)
    state.body:set_unison(2, 0, params.get(4))
    state.bright:set_unison(2, 0, params.get(4))
    state.body:set_highpass(250.0 + brightness * 1250.0, 0.1)
    state.bright:set_highpass(1200.0 + brightness * 5000.0, 0.1)
    state.body_env:set_adsr(0.001, 0.04 + params.get(3) * 0.16, 0.0, 0.015)
    state.bright_env:set_adsr(0.001, 0.01, 0.0, 0.005)

    local gain = voice.velocity * (0.05 + params.get(1) * 0.22)
    state.body:render_env(700.0, host.sample_rate, gain * 0.65, state.body_env)
    state.bright:render_env(3000.0, host.sample_rate, gain * 0.4, state.bright_env)

    if state.body_env:value() < 0.0001 and state.bright_env:value() < 0.0001 then
        voices[voice.id] = nil
        return true
    end
    return false
end

function voice_ended(id)
    voices[id] = nil
end

function draw()
    ui.heading("Clap")
    ui.label("Layered pink/white noise with a wide stereo image")
    ui.param(1, "Level")
    ui.param(2, "Brightness")
    ui.param(3, "Tail")
    ui.param(4, "Width")
end
