-- A plucky FM bass / "donk" patch.
-- Macro 1: level, Macro 2: FM index, Macro 3: pitch drop, Macro 4: phase distortion.

params.set_defaults({
    [1] = 0.65,
    [2] = 0.25,
    [3] = 0.15,
    [4] = 0.1,
})

local voices = {}

function process_voice(voice)
    local state = voices[voice.id]
    if not state then
        local osc = synth.osc("sine")
        osc:set_unison(1)
        state = {
            osc = osc,
            env = synth.adsr(0.002, 0.09, 0.28, 0.08),
            age = voice.age,
            pitch = 1.0,
        }
        state.env:trigger()
        voices[voice.id] = state
    elseif state.age ~= voice.age then
        state.age = voice.age
        state.env:trigger()
        state.pitch = 1.0
    end

    state.env:gate(voice.gate)
    local sample_time = host.num_samples / host.sample_rate

    local pitch_drop_semitones = 7.0 + params.get(3) * 17.0
    local pitch_decay = math.exp(-sample_time / 0.04)
    local frequency = voice.frequency * 2.0 ^ (pitch_drop_semitones * state.pitch / 12.0)
    state.pitch = state.pitch * pitch_decay

    state.osc:set_fm(2.0, 3.0 + params.get(2) * 5.0)
    state.osc:set_pd(params.get(4) * 0.8)
    state.osc:render_env(
        frequency,
        host.sample_rate,
        voice.velocity * (0.05 + params.get(1) * 0.2),
        state.env
    )

    if not voice.gate and state.env:done() then
        voices[voice.id] = nil
        return true
    end
    return false
end

function voice_ended(id)
    voices[id] = nil
end

function draw()
    ui.heading("FM Donk")
    ui.label("Mono or poly: set Voice Mode in the instrument controls")
    ui.param(1, "Level")
    ui.param(2, "FM index")
    ui.param(3, "Pitch drop")
    ui.param(4, "Phase distortion")
    ui.separator()
    ui.label("Sine carrier, 2:1 FM, ADSR envelope, short pitch drop")
end
