-- Transport-synced sixteenth-note arpeggiator.
-- Macro 1: gate length, Macro 2: octave span, Macro 3: direction (up/down).

params.set_defaults({
    [1] = 0.5,
    [2] = 0.0,
    [3] = 0.0,
})

local held = {}
local sequence_index = 0
local next_step = nil
local last_block_end = nil
local fallback_beat = 0.0
local last_reported_beat = nil
local current = nil
local ARP_VOICE_ID = 1

local STEP_BEATS = 0.25

local function find_held(note, channel)
    for index = #held, 1, -1 do
        if held[index].note == note and held[index].channel == channel then
            return index
        end
    end
end

local function release_current(timing)
    if current then
        midi.send_note_off(current.note, current.channel, timing, current.voice_id)
        current = nil
    end
end

function on_midi(event, a, b, c, d)
    if event == "note_on" and b > 0.0 then
        local existing = find_held(a, c)
        if existing then
            held[existing].velocity = b
        else
            held[#held + 1] = { note = a, velocity = b, channel = c }
        end
    elseif event == "note_off" or (event == "note_on" and b <= 0.0) then
        local index = find_held(a, c)
        if index then
            table.remove(held, index)
        end
    elseif event == "cc" then
        midi.send_cc(a, b, c, d)
    elseif event == "pitch_bend" then
        midi.send_pitch_bend(a, c, d)
    elseif event == "channel_pressure" then
        midi.send_channel_pressure(a, c, d)
    end
end

local function make_sequence()
    local notes = {}
    for index, note in ipairs(held) do
        notes[index] = note
    end
    table.sort(notes, function(left, right)
        return left.note < right.note
    end)

    local octaves = 1 + math.floor(params.get(2) * 3.0)
    local sequence = {}
    for octave = 0, octaves - 1 do
        for _, note in ipairs(notes) do
            sequence[#sequence + 1] = {
                note = math.min(127, note.note + octave * 12),
                velocity = note.velocity,
                channel = note.channel,
            }
        end
    end

    if params.get(3) >= 0.5 then
        local reversed = {}
        for index = #sequence, 1, -1 do
            reversed[#reversed + 1] = sequence[index]
        end
        sequence = reversed
    end
    return sequence
end

function process(num_samples, sample_rate, tempo, playing, pos_beats)
    local tempo_value = math.max(1.0, tempo or 120.0)
    local block_beats = num_samples * tempo_value / (60.0 * sample_rate)
    local block_start
    local transport_advanced = playing
        and pos_beats ~= nil
        and (last_reported_beat == nil or math.abs(pos_beats - last_reported_beat) > 1e-9)
    if transport_advanced then
        block_start = pos_beats
    else
        block_start = fallback_beat
    end
    last_reported_beat = playing and pos_beats or nil
    fallback_beat = block_start + block_beats
    local block_end = block_start + block_beats

    if #held == 0 then
        release_current(0)
        next_step = nil
        last_block_end = nil
        last_reported_beat = nil
        return
    end

    if next_step == nil
        or block_start < (last_block_end or block_start) - 0.001
        or block_start > (last_block_end or block_start) + 0.05 then
        next_step = math.ceil((block_start - 1e-9) / STEP_BEATS) * STEP_BEATS
        sequence_index = 0
    end

    local sequence = make_sequence()
    local gate = 0.4 + params.get(1) * 0.55
    local off_beat = current and current.off_beat or math.huge

    while true do
        if current and off_beat < block_end and off_beat <= next_step then
            local timing = math.floor((off_beat - block_start) * 60.0 / tempo_value * sample_rate)
            release_current(math.max(0, math.min(num_samples - 1, timing)))
            off_beat = math.huge
        elseif next_step < block_end then
            if current then
                local timing = math.floor((next_step - block_start) * 60.0 / tempo_value * sample_rate)
                release_current(math.max(0, math.min(num_samples - 1, timing)))
            end

            sequence_index = sequence_index + 1
            local note = sequence[((sequence_index - 1) % #sequence) + 1]
            local timing = math.floor((next_step - block_start) * 60.0 / tempo_value * sample_rate)
            timing = math.max(0, math.min(num_samples - 1, timing))
            midi.send_note_on(note.note, note.velocity, note.channel, timing, ARP_VOICE_ID)
            current = {
                note = note.note,
                channel = note.channel,
                voice_id = ARP_VOICE_ID,
                off_beat = next_step + STEP_BEATS * gate,
            }
            off_beat = current.off_beat
            next_step = next_step + STEP_BEATS
        else
            break
        end
    end

    last_block_end = block_end
end

function draw()
    ui.heading("16th Arpeggiator")
    ui.label("Transport-synced, one octave pattern per held note set")
    ui.param(1, "Gate")
    ui.param(2, "Octave span")
    ui.param(3, "Direction: up / down")
    ui.separator()
    ui.label("Held notes: " .. #held)
end
