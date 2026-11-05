local voice = {}

local function clamp(value, min, max)
    if value < min then return min end
    if value > max then return max end
    return value
end

function voice.process(voice, voices, params)
    local state = voices[voice.id]
    if not state then
        local osc = synth.osc("saw")
        osc:set_lowpass(1200.0, 0.72)
        state = {
            osc = osc,
            env = synth.adsr(0.002, 0.18, 0.0, 0.03),
            age = voice.age,
            current = voice.frequency,
            target = voice.frequency,
            previous = voice.frequency,
            slide_time = 0.0,
            slide_elapsed = 0.0,
            accent = voice.velocity >= 0.8,
        }
        state.env:trigger()
        voices[voice.id] = state
    elseif state.age ~= voice.age then
        state.previous = state.current
        state.target = voice.frequency
        state.age = voice.age
        state.accent = voice.velocity >= 0.8
        state.slide_time = params.get(9)
        state.slide_elapsed = 0.0
        state.osc = synth.osc("saw")
        state.env:trigger()
    end

    local level = params.get(1)
    local tune = params.get(2) * 24.0 - 12.0
    local cutoff = 320.0 + params.get(3) * 1800.0
    local resonance = 0.62 + params.get(4) * 0.33
    local attack = params.get(10)
    local decay = params.get(11)
    local sustain = params.get(12)
    local release = params.get(13)
    local square = params.get(7) >= 0.5
    local accent_amount = params.get(8)
    local slide_time = params.get(9)
    local buffer_time = params.get(14) / 1000.0
    local glide_time = slide_time
    if buffer_time > glide_time then
        glide_time = buffer_time
    end

    if state.slide_time ~= glide_time then
        state.slide_time = glide_time
    end

    if state.slide_time > 0.0 and state.current ~= state.target then
        state.slide_elapsed = state.slide_elapsed + (host.num_samples / host.sample_rate)
        local progress = state.slide_elapsed / state.slide_time
        if progress >= 1.0 then
            state.current = state.target
        else
            state.current = state.previous * (state.target / state.previous) ^ progress
        end
    else
        state.current = state.target
    end

    local frequency = state.current * 2.0 ^ (tune / 12.0)
    local accent_boost = state.accent and (1.0 + accent_amount * 1.25) or 1.0
    local accent_decay = state.accent and (decay * (1.1 + accent_amount * 0.25)) or decay
    state.env:set_adsr(attack, accent_decay, sustain, release)

    state.osc:set_wave(square and "square" or "saw")
    state.osc:set_lowpass(
        cutoff,
        clamp(resonance * (state.accent and 1.06 or 1.0), 0.0, 0.97)
    )
    state.osc:set_lowpass_2(
        cutoff,
        clamp(resonance * (state.accent and 1.06 or 1.0), 0.0, 0.97)
    )

    state.env:gate(voice.gate)

    local gain = voice.velocity * (0.16 + level * 0.5) * accent_boost
    local envelope_depth = (2.5 + params.get(5) * 4.5) * accent_boost
    state.osc:render_filtered(
        frequency,
        host.sample_rate,
        gain,
        state.env,
        nil,
        envelope_depth,
        0.0
    )

    if not voice.gate and state.env:done() then
        voices[voice.id] = nil
        return true
    end
    return false
end

function voice.ended(id, voices)
    voices[id] = nil
end

return voice