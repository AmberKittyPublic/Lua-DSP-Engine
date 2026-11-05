-- Sine pad with two serial low-pass stages.
-- The first cutoff follows a saw LFO at an eighth-note rate for 124 BPM.
-- The second cutoff follows the ADSR envelope.
-- Macro 1: output level, Macro 2: LFO filter depth,
-- Macros 3-6: ADSR, Macro 7: envelope filter depth. LFO depth maps 0..1 to
-- 0..6 octaves; ADSR depth maps 0..1 to 0..4 octaves. Zero disables each sweep.

local voices = {}
local BPM = 124.0
local EIGHTH_NOTE_HZ = BPM / 60.0 * 2.0

params.set_defaults({
    [1] = 0.65,
    [2] = 0.65,
    [3] = 0.01,
    [4] = 0.18,
    [5] = 0.65,
    [6] = 0.22,
    [7] = 0.5,
	[8] = 3, -- type = int
	[9] = 8, -- type = int
	[10]= 0.65
})

function process_voice(voice)
    local state = voices[voice.id]
    if not state then
        local body = synth.osc("sine")
        local harmonics = synth.osc("saw")
        harmonics:set_unison(params.get(8), params.get(9), params.get(10))
        harmonics:set_lowpass(8000, 0.2)
        harmonics:set_lowpass_2(3200, 0.25)
        local attack = params.get(3)
        local decay = params.get(4)
        local sustain = params.get(5)
        local release = params.get(6)
        state = {
            body = body,
            harmonics = harmonics,
            env = synth.adsr(attack, decay, sustain, release),
            filter_env = synth.adsr(attack, decay, sustain, release),
            lfo = synth.lfo("saw", EIGHTH_NOTE_HZ),
            age = voice.age,
        }
        state.lfo:set_sync("1/8d")
        state.lfo:set_tempo(host.tempo or BPM)
        state.lfo:set_inverted(true)
        state.env:trigger()
        state.filter_env:trigger()
        voices[voice.id] = state
    elseif state.age ~= voice.age then
        state.age = voice.age
        state.env:trigger()
        state.filter_env:trigger()
    end

    local attack = params.get(3)
    local decay = params.get(4)
    local sustain = params.get(5)
    local release = params.get(6)
    state.env:set_adsr(attack, decay, sustain, release)
    state.filter_env:set_adsr(attack, decay, sustain, release)
    state.env:gate(voice.gate)
    state.filter_env:gate(voice.gate)
    state.lfo:set_tempo(host.tempo or BPM)
    local gain = voice.velocity * (0.05 + params.get(1) * 0.15)
    local lfo_depth = params.get(2) * 6.0
    local lfo = state.lfo
    if lfo_depth == 0.0 then
        lfo = nil
    end
    state.body:render_env(voice.frequency, host.sample_rate, gain * 0.4, state.env)
    state.harmonics:render_filtered(
        voice.frequency,
        host.sample_rate,
        gain * 0.6,
        state.filter_env,
        lfo,
        params.get(7) * 4.0,
        lfo_depth
    )

    if not voice.gate and state.env:done() and state.filter_env:done() then
        voices[voice.id] = nil
        return true
    end
    return false
end

function voice_ended(id)
    voices[id] = nil
end

function draw()
    ui.heading("Gated Sine Pad")
    ui.label("Sine body plus a filtered saw overtone")
    ui.label("Inverted saw LFO: synced 1/8 (" .. string.format("%.2f", EIGHTH_NOTE_HZ) .. " Hz at 124 BPM)")
    ui.label("Macro 2 controls filter 1; Macro 7 independently controls the ADSR filter sweep")
    ui.label("Set Macro 2 to 0 to stop the LFO; set Macro 7 to 0 to stop the ADSR cutoff sweep")

    ui.row(function()
        ui.param(1, "Output level")
        ui.param(2, "LFO filter depth")
    end)

	ui.separator_horizontal()
    ui.label("OSC 1 Unison")
    ui.row(function()
        ui.param(8, "count")
        ui.separator_vertical()
        ui.param(9, "detune")
    end)
	ui.row(function()
		ui.param(10, "spread")
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

    ui.separator_horizontal()
    ui.param(7, "ADSR filter depth")
    ui.meter(host.meter(), "output peak")
end
