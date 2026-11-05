local voices = {}

params.set_defaults({
    [1] = 1.0,
    [3] = 0.143,
    [4] = 0.045,
    [5] = 0.846,
    [6] = 0.181,
})

local function note_shift()
    return math.floor(params.get(1) * 24.0 + 0.5) - 12
end

function on_midi(event, a, b, c, d)
    if host.kind == "midi_fx" then
        local shift = note_shift()
        if event == "note_on" then
            midi.send_note_on(a + shift, b, c, d)
        elseif event == "note_off" then
            midi.send_note_off(a + shift, c, d)
        elseif event == "cc" then
            midi.send_cc(a, b, c, d)
        elseif event == "pitch_bend" then
            midi.send_pitch_bend(a, c, d)
        elseif event == "channel_pressure" then
            midi.send_channel_pressure(a, c, d)
        end
        return
    end

end

function process(num_samples, sample_rate, tempo, playing, pos_beats)
    if host.kind == "audio_fx" then
        local gain = params.get(1)
        for channel = 0, audio.num_channels() - 1 do
            audio.gain(channel, gain)
        end
    end
end

function process_voice(voice)
    local state = voices[voice.id]
    if not state then
        local osc = synth.osc("saw")
        -- local osc = synth.osc("sine")
        osc:set_unison(3, 8, 0.65)
        -- Try these for other synthesis methods:
        -- osc:set_fm(2.0, 1.5) -- modulator ratio, phase index in radians
        -- osc:set_pd(0.35)     -- phase distortion amount (-0.95..0.95)
        -- osc:set_rm(1.4, 0.5) -- ring-mod ratio, depth (0..1)
        state = {
            osc = osc,
            env = synth.adsr(params.get(3), params.get(4), params.get(5), params.get(6)),
            -- env = synth.adsr(0.005, 0.08, 0.65, 0.12),
            age = voice.age,
        }
        state.env:trigger()
        voices[voice.id] = state
    elseif state.age ~= voice.age then
        state.age = voice.age
        state.env:trigger()
    end

    state.env:gate(voice.gate)
    local gain = (0.05 + params.get(1) * 0.15) * voice.velocity
    state.osc:render_env(voice.frequency, host.sample_rate, gain, state.env)

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
    ui.heading("Lumina DSP")
    ui.label("kind: " .. host.kind)
    ui.label("script: " .. host.script_name)
    ui.separator()

    ui.row(function()
        ui.param(1, "Output level")
        -- ui.param(2, "Macro 2")
    end)

    ui.separator_horizontal()

    ui.label("ADSR envelope")
    ui.row(function()
        ui.param(3, "Attack")
        ui.separator_vertical()
        ui.param(4, "Decay")
    end)
    ui.row(function()
        ui.param(5, "Sustain")
        ui.separator_vertical()
        ui.param(6, "Release")
    end)

    ui.separator()
    ui.meter(host.meter(), "output peak")
    ui.label("Edit the script below, then press Apply & reload.")
end
