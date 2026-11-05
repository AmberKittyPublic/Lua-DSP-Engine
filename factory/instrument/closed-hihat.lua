-- Short closed hi-hat using filtered white noise.
-- Macro 1: level, Macro 2: brightness, Macro 3: decay, Macro 4: width.

local voices = {}

params.set_defaults({
    [1] = 0.65,
    [2] = 0.55,
    [3] = 0.25,
    [4] = 0.45,
})

function process_voice(voice)
    local state = voices[voice.id]
    if not state then
        local noise = synth.osc("white")
        state = {
            noise = noise,
            env = synth.adsr(0.0005, 0.012 + params.get(3) * 0.06, 0.0, 0.004),
            age = voice.age,
        }
        state.env:trigger()
        voices[voice.id] = state
    elseif state.age ~= voice.age then
        state.age = voice.age
        state.env:trigger()
    end

    local decay = 0.012 + params.get(3) * 0.06
    state.env:set_adsr(0.0005, decay, 0.0, 0.004)
    state.noise:set_unison(2, 0, params.get(4))
    state.noise:set_highpass(4500.0 + params.get(2) * 5500.0, 0.15)
    state.noise:set_lowpass(15000.0, 0.05)
    state.noise:render_env(
        8000.0,
        host.sample_rate,
        voice.velocity * (0.04 + params.get(1) * 0.2),
        state.env
    )

    if state.env:value() < 0.0001 then
        voices[voice.id] = nil
        return true
    end
    return false
end

function voice_ended(id)
    voices[id] = nil
end

function draw()
    ui.heading("Closed Hi-Hat")
    ui.label("Short, filtered white-noise hit")
    ui.param(1, "Level")
    ui.param(2, "Brightness")
    ui.param(3, "Decay")
    ui.param(4, "Width")
end
