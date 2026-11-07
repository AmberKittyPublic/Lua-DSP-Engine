-- 12-pitch by variable-length note sequencer.
-- Macros 1-N (N = steps): 12-bit pitch masks (0 = ARP/rest). Macro N+1: steps
-- (4/8/16/32). Macros after that: accent, skip, and global params including
-- a live/MIDI-trigger mode toggle.
-- 12 rows = all chromatic notes.

local rates = { "1/16", "1/16 dotted", "1/16 triplet", "1/8", "1/8 dotted", "1/8 triplet", "1/4", "1/4 dotted", "1/4 triplet", "1/2", "1/2 dotted", "1/2 triplet", "1/1", "1/32", "1/32 dotted", "1/32 triplet", "1/64" }
local rate_beats = { 0.25, 0.375, 1.0 / 6.0, 0.5, 0.75, 1.0 / 3.0, 1.0, 1.5, 2.0 / 3.0, 2.0, 3.0, 4.0 / 3.0, 4.0, 0.125, 0.1875, 1.0 / 12.0, 0.0625 }
local directions = { "Up", "Down", "Up / Down", "Down / Up", "Converge", "Diverge", "Random", "As Played" }
local velocity_modes = { "Input x Step", "Fixed x Step", "Random x Step" }
local key_names = { "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B" }
local step_lengths = { "4", "8", "16", "32" }
local pitch_offsets = { 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11 }

local step_scale = { type = "float", min = 0.0, max = 1.0 }
local integer_scales = {
    [33] = { type = "int", min = 0, max = 3 },
    [34] = { type = "int", min = 0, max = 7 },
    [35] = { type = "int", min = 0, max = 7 },
    [36] = { type = "int", min = 1, max = 4 },
    [37] = { type = "float", min = 40.0, max = 100.0 },
    [38] = { type = "float", min = 0.0, max = 33.0 },
    [39] = { type = "int", min = 0, max = 24 },
    [40] = { type = "int", min = 1, max = 4 },
    [41] = { type = "int", min = 0, max = 1 },
    [42] = { type = "int", min = 0, max = 1 },
    [43] = { type = "int", min = 0, max = 1 },
    [44] = { type = "int", min = 0, max = 11 },
    [45] = { type = "int", min = 0, max = 2 },
    [46] = { type = "float", min = 0.5, max = 1.0 },
    [47] = { type = "float", min = 0.0, max = 25.0 },
    [48] = { type = "float", min = 0.0, max = 100.0 },
    [49] = { type = "int", min = 0, max = 24 },
    [50] = { type = "float", min = 0.0, max = 100.0 },
    [51] = { type = "float", min = 0.0, max = 100.0 },
}

for index = 1, 32 do
    params.set_range(index, { type = "int", min = 0, max = 4095 })
end
for index = 33, 51 do
    if integer_scales[index] then
        params.set_range(index, integer_scales[index])
    end
end

local defaults = {
    [33] = 2, [34] = 0, [35] = 0, [36] = 1, [37] = 80.0, [38] = 0.0,
    [39] = 0, [40] = 1, [41] = 0, [42] = 0, [43] = 1, [44] = 0,
    [45] = 0, [46] = 0.8, [47] = 0.0, [48] = 0.0, [49] = 0,
    [50] = 0.0, [51] = 0.0,
}
for step = 1, 32 do
    defaults[step] = 0.0
end
params.set_defaults(defaults)

local held = {}
local active_outputs = {}
local pending_ons = {}
local next_step_beat = nil
local fallback_beat = 0.0
local last_block_end = nil
local step_position = 0
local arp_position = 0
local next_voice_id = 0
local previous_rate = nil
local was_playing = false
local reset_arp_pending = false

local function find_held(note, channel)
    for index = #held, 1, -1 do
        if held[index].note == note and held[index].channel == channel then
            return index
        end
    end
end

local function release_all(timing)
    for index = #active_outputs, 1, -1 do
        local active = table.remove(active_outputs, index)
        midi.send_note_off(active.note, active.channel, timing, active.voice_id)
    end
    pending_ons = {}
end

function on_midi(event, a, b, c, d)
    if event == "note_on" and b > 0.0 then
        local existing = find_held(a, c)
        if existing then
            held[existing].velocity = b
            held[existing].released = false
        else
            held[#held + 1] = { note = a, velocity = b, channel = c, released = false }
        end
        reset_arp_pending = true
    elseif event == "note_off" or event == "choke" or (event == "note_on" and b <= 0.0) then
        local index = find_held(a, c)
        if index then
            reset_arp_pending = true
            if params.get(41) >= 0.5 then
                held[index].released = true
            else
                table.remove(held, index)
            end
        end
    elseif event == "cc" then
        midi.send_cc(a, b, c, d)
    elseif event == "pitch_bend" then
        midi.send_pitch_bend(a, c, d)
    elseif event == "channel_pressure" then
        midi.send_channel_pressure(a, c, d)
    end
end

local function make_pitch_sequence(direction, octave_count)
    local notes = {}
    for index, note in ipairs(held) do
        notes[index] = note
    end
    if direction ~= 7 then
        table.sort(notes, function(left, right)
            return left.note < right.note
        end)
    end

    local pitches = {}
    for octave = 0, octave_count - 1 do
        for _, note in ipairs(notes) do
            local pitch = note.note + octave * 12
            if pitch <= 127 then
                pitches[#pitches + 1] = {
                    note = pitch,
                    velocity = note.velocity,
                    channel = note.channel,
                }
            end
        end
    end

    local count = #pitches
    if direction == 1 then
        local reversed = {}
        for index = count, 1, -1 do
            reversed[#reversed + 1] = pitches[index]
        end
        return reversed
    elseif direction == 2 or direction == 3 then
        local walked = {}
        if direction == 2 then
            for index = 1, count do walked[#walked + 1] = pitches[index] end
            for index = count - 1, 2, -1 do walked[#walked + 1] = pitches[index] end
        else
            for index = count, 1, -1 do walked[#walked + 1] = pitches[index] end
            for index = 2, count - 1 do walked[#walked + 1] = pitches[index] end
        end
        return #walked > 0 and walked or pitches
    elseif direction == 4 then
        local walked = {}
        local low, high = 1, count
        while low <= high do
            walked[#walked + 1] = pitches[low]
            if low ~= high then walked[#walked + 1] = pitches[high] end
            low, high = low + 1, high - 1
        end
        return walked
    elseif direction == 5 then
        local walked = {}
        local center = math.floor((count + 1) / 2)
        local distance = 0
        while #walked < count do
            local low = center - distance
            local high = center + distance
            if low >= 1 and #walked < count then walked[#walked + 1] = pitches[low] end
            if distance > 0 and high <= count and #walked < count then
                walked[#walked + 1] = pitches[high]
            end
            distance = distance + 1
        end
        return walked
    end
    return pitches
end

local function new_voice_id()
    next_voice_id = next_voice_id + 1
    if next_voice_id > 2147483647 then
        next_voice_id = 1
    end
    return next_voice_id
end

local function sample_timing(beat, block_start, tempo, sample_rate, num_samples)
    local timing = math.floor((beat - block_start) * 60.0 / tempo * sample_rate + 0.5)
    return math.max(0, math.min(num_samples - 1, timing))
end

local function push_event(events, kind, beat, note, channel, velocity, voice_id, off_beat)
    events[#events + 1] = {
        kind = kind,
        beat = beat,
        note = note,
        channel = channel,
        velocity = velocity,
        voice_id = voice_id,
        off_beat = off_beat,
    }
end

local function add_note(events, note, step_velocity, on_beat, substep_beats,
        ratchets, gate, velocity_mode, fixed_velocity, velocity_humanize, block_end)
    local velocity
    if velocity_mode == 1 then
        velocity = fixed_velocity
    else
        velocity = note.velocity
    end
    velocity = velocity * step_velocity
    if velocity_mode == 2 then
        velocity = velocity * (0.5 + math.random() * 0.5)
    end
    if velocity_humanize > 0.0 then
        velocity = velocity * (1.0 + (math.random() * 2.0 - 1.0) * velocity_humanize / 100.0)
    end
    velocity = math.max(0.01, math.min(1.0, velocity))

    local transpose = params.get(39) - params.get(49)
    for ratchet = 0, ratchets - 1 do
        local beat = on_beat + ratchet * substep_beats
        local off_beat = beat + substep_beats * gate
        local output_note = math.max(0, math.min(127, note.note + transpose))
        local voice_id = new_voice_id()
        if beat < block_end then
            push_event(events, "on", beat, output_note, note.channel, velocity, voice_id, off_beat)
            if off_beat < block_end then
                push_event(events, "off", off_beat, output_note, note.channel, nil, voice_id, off_beat)
            end
        else
            pending_ons[#pending_ons + 1] = {
                beat = beat,
                off_beat = off_beat,
                note = output_note,
                channel = note.channel,
                velocity = velocity,
                voice_id = voice_id,
            }
        end
    end
end

function process(num_samples, sample_rate, tempo, playing, pos_beats)
    if sample_rate <= 0.0 then
        return
    end
    if num_samples <= 0 then
        release_all(0)
        next_step_beat = nil
        last_block_end = nil
        return
    end

    local tempo_value = math.max(1.0, tempo or 120.0)
    local block_beats = num_samples * tempo_value / (60.0 * sample_rate)
    local block_start = playing and pos_beats ~= nil and pos_beats or fallback_beat
    local block_end = block_start + block_beats
    fallback_beat = block_end

    if params.get(41) < 0.5 then
        for index = #held, 1, -1 do
            if held[index].released then
                table.remove(held, index)
            end
        end
    end

    if #held == 0 then
        local midi_trigger = params.get(42) >= 0.5
        local length_index = params.get(33)
        local num_steps = ({ 4, 8, 16, 32 })[length_index + 1]
        local has_explicit = false
        if not midi_trigger then
            for step = 1, num_steps do
                if params.get(step) > 0 then
                    has_explicit = true
                    break
                end
            end
        end
        if not has_explicit then
            release_all(0)
            next_step_beat = nil
            last_block_end = nil
            was_playing = playing
            return
        end
    end

    local rate_index = math.max(0, math.min(16, math.floor(params.get(34) + 0.5)))
    local step_beats = rate_beats[rate_index + 1]
    local jumped = last_block_end ~= nil and (
        block_start < last_block_end - 1e-5 or block_start > last_block_end + math.max(block_beats * 2.0, 0.05)
    )
    local transport_reset = playing and not was_playing and params.get(43) >= 0.5
    if next_step_beat == nil or jumped or rate_index ~= previous_rate or transport_reset then
        release_all(0)
        next_step_beat = math.ceil((block_start - 1e-9) / step_beats) * step_beats
        step_position = 0
        arp_position = 0
    elseif reset_arp_pending then
        arp_position = 0
        pending_ons = {}
    end
    previous_rate = rate_index
    was_playing = playing
    reset_arp_pending = false

    local direction = math.max(0, math.min(7, math.floor(params.get(35) + 0.5)))
    local octave_count = params.get(36)
    local sequence = make_pitch_sequence(direction, octave_count)
    local length_index = params.get(33)
    local pattern_length = ({ 4, 8, 16, 32 })[length_index + 1]
    local gate = params.get(37) / 100.0
    local swing = params.get(38) / 100.0
    local ratchets = params.get(40)
    local velocity_mode = params.get(45)
    local fixed_velocity = params.get(46)
    local timing_humanize = params.get(47) / 100.0
    local velocity_humanize = params.get(48)
    local key = params.get(44)
    local events = {}

    for index = #pending_ons, 1, -1 do
        local pending = pending_ons[index]
        if pending.beat < block_end then
            push_event(events, "on", pending.beat, pending.note, pending.channel,
                pending.velocity, pending.voice_id, pending.off_beat)
            if pending.off_beat < block_end then
                push_event(events, "off", pending.off_beat, pending.note, pending.channel,
                    nil, pending.voice_id, pending.off_beat)
            end
            table.remove(pending_ons, index)
        end
    end

    for index = #active_outputs, 1, -1 do
        local active = active_outputs[index]
        if active.off_beat < block_end then
            push_event(events, "off", math.max(block_start, active.off_beat),
                active.note, active.channel, nil, active.voice_id, active.off_beat)
            table.remove(active_outputs, index)
        end
    end

    local steps_processed = 0
    while next_step_beat < block_end and steps_processed < 256 do
        steps_processed = steps_processed + 1
        local step_number = step_position % pattern_length + 1
        local on_beat = next_step_beat
        if step_number % 2 == 0 then
            on_beat = on_beat + step_beats * swing
        end
        if timing_humanize > 0.0 then
            on_beat = on_beat + (math.random() * 2.0 - 1.0) * step_beats * timing_humanize
        end
        on_beat = math.max(next_step_beat, math.min(next_step_beat + step_beats * 0.95, on_beat))
        if on_beat >= block_end then
            break
        end
        step_position = step_position + 1

        local step_mask = math.floor(params.get(step_number) + 0.5)
        local step_notes = {}
        if step_mask > 0 then
            for lane = 0, 11 do
                if math.floor(step_mask / (2 ^ lane)) % 2 == 1 then
                    step_notes[#step_notes + 1] = {
                        note = 60 + key + lane + (octave_count - 1) * 12,
                        velocity = (held[1] and held[1].velocity) or fixed_velocity,
                        channel = (held[1] and held[1].channel) or 0,
                    }
                end
            end
        elseif #sequence > 0 then
            arp_position = arp_position + 1
            if direction == 6 then
                step_notes[1] = sequence[math.random(1, #sequence)]
            else
                step_notes[1] = sequence[((arp_position - 1) % #sequence) + 1]
            end
        end

        local skip_chance = params.get(51)
        if #step_notes > 0 and math.random() >= skip_chance then
            local step_velocity = 1.0 + params.get(50) * 0.5
            local substep_beats = step_beats / ratchets
            for _, note in ipairs(step_notes) do
                add_note(events, note, step_velocity, on_beat, substep_beats,
                    ratchets, gate, velocity_mode, fixed_velocity, velocity_humanize, block_end)
            end
        end
        next_step_beat = next_step_beat + step_beats
    end

    table.sort(events, function(left, right)
        if left.beat == right.beat then
            return left.kind == "off" and right.kind ~= "off"
        end
        return left.beat < right.beat
    end)

    for _, event in ipairs(events) do
        local timing = sample_timing(event.beat, block_start, tempo_value, sample_rate, num_samples)
        if event.kind == "on" then
            midi.send_note_on(event.note, event.velocity, event.channel, timing, event.voice_id)
            if event.off_beat >= block_end then
                active_outputs[#active_outputs + 1] = {
                    note = event.note,
                    channel = event.channel,
                    voice_id = event.voice_id,
                    off_beat = event.off_beat,
                }
            end
        elseif event.beat < block_end then
            midi.send_note_off(event.note, event.channel, timing, event.voice_id)
        end
    end

    host.trace((step_position % pattern_length) / pattern_length)
    last_block_end = block_end
end

function draw()
    local pattern = {}
    local chance = {}
    local explicit_count = 0
    local num_steps = ({ 4, 8, 16, 32 })[params.get(33) + 1]
    local global_skip = params.get(51)
    for step = 1, num_steps do
        local mask = math.floor(params.get(step) + 0.5)
        pattern[step] = mask == 0 and 0.06 or 0.35 + math.log(mask + 1) / math.log(4096) * 0.55
        chance[step] = 0.5 + (1.0 - global_skip) * 0.48
        if mask > 0 then explicit_count = explicit_count + 1 end
    end

    local key = params.get(44)
    local rows = {}
    for lane = 12, 1, -1 do
        rows[#rows + 1] = key_names[(key + pitch_offsets[lane]) % 12 + 1]
    end

    ui.colored_heading("12 x " .. num_steps .. " NOTE GRID", "#61d6c3")
    ui.label("Click cells to toggle the 12 chromatic pitches. Empty steps use the held chord (ARP) in live mode.")
    ui.note_grid(1, rows, num_steps, "#61d6c3")

    ui.row(function()
        ui.column(function()
            ui.group("Clock and pitch", function()
                ui.knob(34, "Synced rate", integer_scales[34])
                ui.label("Rate: " .. rates[math.floor(params.get(34) + 0.5) + 1])
                ui.radio_param(35, "Note order", directions)
                ui.radio_param(44, "Key / root", key_names)
                ui.row(function()
                    ui.knob(36, "Octaves")
                    ui.knob(37, "Gate", integer_scales[37])
                    ui.knob(38, "Swing", integer_scales[38])
                end)
                ui.row(function()
                    ui.knob(39, "Transpose up")
                    ui.knob(49, "Transpose down")
                    ui.knob(40, "Ratchets")
                    ui.select(33, "Steps", step_lengths)
                    ui.toggle(42, "MIDI trigger mode")
                    ui.toggle(41, "Latch held notes")
                end)
            end)
            ui.group("Performance", function()
                ui.radio_param(45, "Velocity mode", velocity_modes)
                ui.row(function()
                    if params.get(45) == 1 then
                        ui.knob(46, "Fixed velocity", integer_scales[46])
                    end
                    ui.knob(47, "Timing humanize", integer_scales[47])
                    ui.knob(48, "Velocity humanize", integer_scales[48])
                    ui.toggle(43, "Reset on transport start")
                end)
                ui.row(function()
                    ui.param(50, "Accent", step_scale)
                    ui.param(51, "Skip chance", step_scale)
                end)
            end)
        end)
        ui.column(function()
            ui.group("Pattern display", function()
                ui.graph(function()
                    ui.plot(pattern, "#61d6c3")
                    ui.plot(chance, "#f0ad72")
                    ui.plot(host.gain_trace(), "#c678dd")
                end)
                ui.label(string.format("%d explicit steps / %d-step loop", explicit_count, num_steps))
                ui.label("Teal: pitch density | orange: chance | purple: playhead")
            end)
        end)
    end)
end