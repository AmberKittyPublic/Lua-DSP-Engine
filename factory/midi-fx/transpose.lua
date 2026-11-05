-- MIDI transposer with discrete octave radios and a continuous semitone macro.
-- Change OCTAVE_RANGE below to set the octave radio range.

local OCTAVE_RANGE = 3
local active_notes = {}
local next_voice_id = 0

params.set_defaults({
    [1] = 0.5,
    [2] = 0.5,
})

local function transposition()
    local octave_index = math.floor(params.get(1) * (OCTAVE_RANGE * 2) + 0.5)
    local octaves = octave_index - OCTAVE_RANGE
    local semitones = math.floor(params.get(2) * 24.0 + 0.5) - 12
    return octaves * 12 + semitones
end

local function find_active_note(note, channel)
    for index = #active_notes, 1, -1 do
        local active = active_notes[index]
        if active.input_note == note and active.channel == channel then
            return index
        end
    end
end

local function note_off(note, channel, timing)
    local index = find_active_note(note, channel)
    if not index then
        return
    end

    local active = table.remove(active_notes, index)
    midi.send_note_off(active.output_note, active.channel, timing, active.voice_id)
end

local function new_voice_id()
    next_voice_id = next_voice_id + 1
    if next_voice_id > 2_147_483_647 then
        next_voice_id = 1
    end
    return next_voice_id
end

function on_midi(event, a, b, c, d)
    if (event == "note_on" and b <= 0.0) or event == "note_off" or event == "choke" then
        note_off(a, c, d)
    elseif event == "note_on" then
        local output_note = math.max(0, math.min(127, a + transposition()))
        local voice_id = new_voice_id()
        active_notes[#active_notes + 1] = {
            input_note = a,
            output_note = output_note,
            channel = c,
            voice_id = voice_id,
        }
        midi.send_note_on(output_note, b, c, d, voice_id)
    elseif event == "cc" then
        midi.send_cc(a, b, c, d)
    elseif event == "pitch_bend" then
        midi.send_pitch_bend(a, c, d)
    elseif event == "channel_pressure" then
        midi.send_channel_pressure(a, c, d)
    end
end

local function octave_choices()
    local choices = {}
    for octave = -OCTAVE_RANGE, OCTAVE_RANGE do
        choices[#choices + 1] = octave > 0 and ("+" .. octave) or tostring(octave)
    end
    return choices
end

function draw()
    ui.heading("MIDI Transposer")
    ui.label("Octaves use radios; semitones use a slider.")
    ui.radio_param(1, "Octaves", octave_choices())
    ui.param(2, "Semitones")
    ui.separator()
    ui.label("Octave range: -" .. OCTAVE_RANGE .. " to +" .. OCTAVE_RANGE)
    ui.label("Semitone range: -12 to +12")
end
