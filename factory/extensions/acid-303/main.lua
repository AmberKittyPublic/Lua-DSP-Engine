-- Acid 303: a modular TB-303-inspired mono synth.
-- Macro 1: level, 2: tune, 3: cutoff, 4: resonance, 5: envelope modulation,
-- 7: square wave, 8: accent, 9: slide time.
-- Macros 10-13: ADSR attack, decay, sustain, and release.

synth.set_mode("mono")
synth.set_max_voices(1)

local voice = require("voice")
local ui = require("ui")

local ATTACK_SCALE = { type = "float", min = 0.001, max = 0.1 }
local DECAY_SCALE = { type = "float", min = 0.01, max = 1.0 }
local SUSTAIN_SCALE = { type = "float", min = 0.0, max = 1.0 }
local RELEASE_SCALE = { type = "float", min = 0.01, max = 1.0 }
local BUFFER_SCALE = { type = "float", min = 0.0, max = 100.0 }

params.set_range(10, ATTACK_SCALE)
params.set_range(11, DECAY_SCALE)
params.set_range(12, SUSTAIN_SCALE)
params.set_range(13, RELEASE_SCALE)
params.set_range(14, BUFFER_SCALE)

params.set_defaults({
    [1] = 0.75,
    [2] = 0.5,
    [3] = 0.45,
    [4] = 0.75,
    [5] = 0.7,
    [7] = 0.0,
    [8] = 0.7,
    [9] = 0.08,
    [10] = 0.002,
    [14] = 0.0,
    [11] = 0.18,
    [12] = 0.0,
    [13] = 0.03,
})

local voices = {}

function process_voice(v)
    return voice.process(v, voices, params)
end

function voice_ended(id)
    voice.ended(id, voices)
end

function draw()
    ui.draw(params)
end