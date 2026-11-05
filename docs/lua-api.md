# Lua API Reference

Scripts run in Luau. API functions and tables described here are installed as
globals. All array indices for audio channels, audio samples, MIDI channels, macro
parameters, and MIDI notes are explicitly noted below; most are zero-based except
macro parameters.

## Plugin Kinds

`host.kind` is one of:

- `"instrument"`: MIDI instrument with mono/poly voice management.
- `"audio_fx"`: audio effect with optional sidechain input.
- `"midi_fx"`: MIDI input/output effect with a per-block `process()` callback.

All kinds expose `host.role`, which is `"audio"` for the processing Lua state and
`"ui"` for the UI Lua state. The states are independent; their local Lua data is
not shared.

## Callback Summary

| Callback | Instrument | Audio FX | MIDI FX | Purpose |
| --- | --- | --- | --- | --- |
| `on_midi(event, a, b, c, d)` | MIDI events | Not called | MIDI events | Handle input MIDI. |
| `process(n, sr, tempo, playing, pos_beats)` | Fallback if no `process_voice` | Audio block | MIDI block | General block processing. |
| `process_voice(voice)` | Once per active voice | Not used | Not used | Render an instrument voice; return `true` when finished. |
| `voice_ended(id)` | On voice removal | Not used | Not used | Clean up script state for a finished/stolen voice. |
| `draw()` | UI frame | UI frame | UI frame | Build the script-defined UI. |

`process_voice()` takes precedence over `process()` for instrument audio. MIDI FX
calls `on_midi()` for input events and then calls `process()` for the block.

### Block Callback Arguments

`process(num_samples, sample_rate, tempo, playing, pos_beats)` receives:

- `num_samples`: samples in this processing block.
- `sample_rate`: samples per second.
- `tempo`: beats per minute, or `nil` if unavailable.
- `playing`: transport playback state.
- `pos_beats`: song position in quarter-note beats, or `nil` if unavailable.

The same values are available through the corresponding `host` fields. In a
MIDI-FX callback, use `midi.send_*()` to queue output events. The `audio` buffer is
not available in MIDI-FX processing.

### Voice Callback

`process_voice(voice)` receives a table with:

| Field | Meaning |
| --- | --- |
| `voice.id` | Host-local stable identifier for this Lua voice. |
| `voice.note` | MIDI note number, `0..127`. |
| `voice.channel` | MIDI channel, `0..15`. |
| `voice.velocity` | Note velocity, normalized `0..1`. |
| `voice.gate` | `true` while held, then `false` after note-off. |
| `voice.frequency` | Equal-tempered note frequency in Hz (`A4 = 440 Hz`). |
| `voice.age` | Monotonically increasing note-change serial used for retrigger detection. |

When `gate` becomes false, keep returning `false` while rendering the release tail.
Return `true` when the voice is silent and can be removed. Define `voice_ended(id)`
to discard Lua-owned state when a voice is stolen, choked, or otherwise removed.
The voice allocator applies note changes at block boundaries; `process_voice()` does
not currently receive the note event's sample offset. Use `on_midi()`'s `d` timing
argument for scripts that need to schedule around input event offsets.

Example:

```lua
local voices = {}

function process_voice(voice)
    local state = voices[voice.id]
    if not state then
        state = {
            osc = synth.osc("saw"),
            env = synth.adsr(0.005, 0.1, 0.65, 0.2),
            age = voice.age,
        }
        state.env:trigger()
        voices[voice.id] = state
    elseif state.age ~= voice.age then
        state.age = voice.age
        state.env:trigger()
    end

    state.env:gate(voice.gate)
    state.osc:render_env(
        voice.frequency,
        host.sample_rate,
        voice.velocity * 0.2,
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
```

## `host`

| Field/member | Meaning |
| --- | --- |
| `host.kind` | Plugin kind string. |
| `host.role` | `"audio"` or `"ui"`. |
| `host.script_name` | Current script name, such as `user.lua`. |
| `host.sample_rate` | Current sample rate in Hz. |
| `host.num_samples` | Current block length in samples. |
| `host.tempo` | Current BPM or `nil`. |
| `host.playing` | Whether transport is running. |
| `host.pos_beats` | Song position in quarter-note beats or `nil`. |
| `host.voice_mode` | Effective instrument mode: `"mono"` or `"poly"`. |
| `host.max_voices` | Effective maximum voice count. |
| `host.meter()` | Last measured audio output peak in linear amplitude; it may exceed `1.0`. MIDI FX does not produce this meter. |

Transport fields are updated for processing callbacks. Do not assume a meaningful
`host.num_samples` or transport position during `draw()`.

## `params`

- `params.count()` returns `64`.
- `params.get(index)` reads a macro value in `0..1`.
- `params.default(index)` reads the current script-declared reset value.
- `params.set_default(index, value)` declares one script reset value.
- `params.set_defaults({ [index] = value, ... })` declares several at once.

### Macro Defaults

Declare defaults at the top level of the script:

```lua
params.set_default(1, 0.65)
params.set_defaults({
    [2] = 0.25,
    [3] = 0.1,
})
```

`params.default(index)` reads the declared reset value. Unspecified macros default
to `0`. Indices are **1-based** (`1..64`); values must be finite numbers in
`0..1`. Declarations are evaluated when the script loads and should not vary by
`host.role`, because audio and UI Lua states share this metadata. Calls from later
callbacks do not change the defaults.

Declaring a default does **not** change the current macro value or the DAW's native
parameter default. Applying or reloading a script preserves current and automated
values. In the **Script** panel, press **Reset macros to script defaults** to apply
the script's declared values to the actual plugin parameters.

Macro indexing is **1-based**. An index outside `1..64` raises a Lua error. Macro
values are read-only from Lua and can be automated by the host.

## `audio`

Audio channel and sample indices are **0-based**. Out-of-range reads return `nil`;
out-of-range writes are ignored.

- `audio.num_samples()` returns the current block size.
- `audio.num_channels()` returns the main audio channel count.
- `audio.get(channel, sample)` reads a sample or returns `nil`.
- `audio.set(channel, sample, value)` replaces a sample.
- `audio.add(channel, sample, value)` adds to a sample, useful for mixing voices.
- `audio.gain(channel, gain)` multiplies every sample in a channel by `gain`.
- `audio.sc_num_channels()` returns the channel count on the first sidechain input.
- `audio.sc_get(channel, sample)` reads a sidechain sample or returns `nil`.

Instrument output is cleared before each block. Audio-FX buffers contain the
incoming audio. `audio.sc_get()` is read-only and is useful in the Audio FX plugin;
it returns `nil` if the requested sidechain sample is unavailable.

Example gain effect:

```lua
function process(num_samples, sample_rate, tempo, playing, pos_beats)
    local gain = params.get(1)
    for channel = 0, audio.num_channels() - 1 do
        audio.gain(channel, gain)
    end
end
```

## `midi`

MIDI note numbers and channels are **0-based**. Event `timing` values are sample
offsets within the current block and default to `0` when omitted. Sending events
does not automatically pass through input events; scripts must explicitly forward
events they want to preserve.

`on_midi(event, a, b, c, d)` receives these currently supported event strings:

| `event` | `a` | `b` | `c` | `d` |
| --- | --- | --- | --- | --- |
| `"note_on"` | note | velocity `0..1` | channel | sample timing |
| `"note_off"` | note | release velocity | channel | sample timing |
| `"choke"` | note | `0` | channel | sample timing |
| `"cc"` | CC number | normalized value `0..1` | channel | sample timing |
| `"pitch_bend"` | normalized value `0..1` (`0.5` center) | `0` | channel | sample timing |
| `"channel_pressure"` | normalized pressure `0..1` | `0` | channel | sample timing |

The MIDI FX receives note events, CC, pitch bend, and channel pressure. The
instrument currently receives basic note events (`note_on`, `note_off`, and
`choke`); the audio FX does not receive MIDI input.

Send functions:

- `midi.send_note_on(note, velocity, channel?, timing?, voice_id?)`
- `midi.send_note_off(note, channel?, timing?, voice_id?)`
- `midi.send_cc(cc, value, channel?, timing?)`
- `midi.send_pitch_bend(value, channel?, timing?)`
- `midi.send_channel_pressure(pressure, channel?, timing?)`

`voice_id` is optional and must be a non-negative integer when provided. Without
one, the event uses a wildcard ID. Use the same explicit ID for matching note-on
and note-off events, especially when supporting overlapping notes. Note velocity,
CC, pitch-bend, and pressure values are normalized to `0..1`; pitch bend is
centered at `0.5`. MIDI output is limited to 1024 queued events per block.

## `wave`

Waveform functions take **phase in turns**: `0` and `1` are the same phase, and
values outside `0..1` wrap. Basic waves return values around `-1..1`.

- `wave.sine(phase)` and `wave.sin(phase)` return a sine wave.
- `wave.saw(phase)` ramps from `-1` to `1` over one cycle.
- `wave.square(phase)` is `1` for the first half-cycle and `-1` for the second.
- `wave.triangle(phase)` rises from `-1` to `1`, then falls to `-1`.
- `wave.pulse(phase, width?)` is `1` before `width`, then `-1`; width defaults to `0.5` and is clamped to `0..1`.
- `wave.noise()` returns a new noise sample in `-1..1`.
- `wave.fm(carrier_phase, modulator, index)` returns a sine carrier with phase modulation; `index` is in radians and `modulator` is a signal value.
- `wave.pd(phase, amount)` returns a phase-distorted sine; amount is clamped to `-0.95..0.95`.
- `wave.rm(carrier, modulator)` multiplies two signal values.
- `wave.am(carrier, modulator, depth?)` applies amplitude modulation; depth defaults to `1` and is clamped to `0..1`.

For full-block synthesis, prefer `synth.osc()` to calling waveform functions in a
per-sample Lua loop.

## `synth`

### Voice settings

- `synth.note_freq(midi_note)` converts a MIDI note number to equal-tempered Hz.
- `synth.set_mode("mono")` or `synth.set_mode("poly")` overrides the Voice Mode parameter.
- `synth.set_mode("default")` or `synth.set_mode(nil)` clears that override.
- `synth.set_max_voices(count)` overrides Max Voices; the value is clamped to `1..32`.
- `synth.set_max_voices(nil)` clears the override.

These controls are used by the instrument; the other plugin types ignore them.
Overrides are shared with the host voice allocator and are reinitialized when a
script is loaded. The UI/host parameter remains the fallback setting.

### Oscillators

Create an oscillator with `synth.osc(waveform)`. Waveform names are `"sine"`,
`"sin"`, `"saw"`, `"square"`, `"triangle"`, `"white"`, `"pink"`, and `"brown"`.
`"noise"` is an alias for `"white"`. Noise waveforms generate a fresh sample in
Rust for every output sample; oscillator frequency does not affect their spectrum.

- `osc:set_wave(waveform)` changes its waveform.
- `osc:set_unison(count, detune_cents?, stereo_spread?)` configures 1..16 voices. Detune is the maximum edge deviation in cents (clamped to ±100); stereo spread is `0..1`.
- `osc:set_fm(modulator_ratio, index_radians)` enables internal sine phase modulation. Ratio is clamped to `0..32`; index is clamped to `0..32` radians.
- `osc:set_pd(amount)` configures phase distortion, clamped to `-0.95..0.95`.
- `osc:set_rm(modulator_ratio, depth)` configures internal sine ring modulation; ratio is `0..32`, depth is `0..1`.
- `osc:set_lowpass(cutoff_hz, resonance)` enables a two-pole low-pass filter. Resonance is normalized to `0..1`.
- `osc:set_lowpass_2(cutoff_hz, resonance)` enables a second low-pass stage in series.
- `osc:clear_filter()` disables the filter and clears its state.
- `osc:set_pitch_drop(semitones, decay_seconds)` starts a Rust-rendered downward pitch sweep for subsequent blocks. It begins at `frequency_hz * 2^(semitones / 12)` and exponentially falls to the requested frequency. Semitones clamp to `0..48`, decay to `0.001..5` seconds. Call it once when triggering a note, not every block.
- `osc:render(frequency_hz, sample_rate, gain?)` renders the current block and adds the result to the output buffer. Gain is linear and defaults to `1`.
- `osc:render_env(frequency_hz, sample_rate, gain, envelope)` renders with the envelope applied per sample in Rust. It advances that envelope; do not also call `envelope:process()` for the same samples.
- `osc:render_filtered(frequency_hz, sample_rate, gain, envelope?, lfo?, envelope_depth_octaves?, lfo_depth_octaves?)` renders through enabled low-pass stages. The envelope shapes amplitude and modulates the second stage's cutoff; the LFO modulates the first stage's cutoff. Depths are in octaves and clamp to `-8..8`.

Unison voices are normalized and panned across the first two output channels. A
mono output receives a summed signal. Additional channels receive the mono sum.

### LFOs

`local lfo = synth.lfo(waveform, rate_hz)` creates an LFO with phase zero. It
supports `"sine"`, `"sin"`, `"saw"`, `"square"`, and `"triangle"` (not `"noise"`).

- `lfo:set_rate(rate_hz)` changes the rate, clamped to `0..20000` Hz.
- `lfo:set_sync(division)` syncs the LFO to a note division: `"1/1"`, `"1/2"`, `"1/4"`, `"1/8"`, `"1/16"`, `"1/32"`, or `"1/64"`. Append `d` for dotted or `t` for triplet (for example, `"1/8d"` or `"1/8t"`).
- `lfo:set_tempo(bpm)` sets the tempo used by synced mode. Update it with `host.tempo` while processing; it defaults to `120` BPM.
- `lfo:rate()` returns the current effective rate in Hz. Calling `set_rate()` switches back to free-running mode.
- `lfo:reset(phase?)` resets phase in turns; it defaults to `0`.
- `lfo:set_inverted(bool)` flips the waveform polarity.
- `lfo:value()` reads the waveform value at the current phase without advancing.
- `lfo:process(num_samples, sample_rate)` advances across the block and returns the block-average waveform value.

A synced division is measured in quarter-note beats: `rate_hz = BPM / 60 / beats_per_cycle`.
For example, `"1/8"` is half a quarter-note beat, or about `4.13 Hz` at `124 BPM`.

The block-average value is suitable for control-rate modulation, such as gently
modulating a delay time. For audio-rate oscillator modulation, use
`osc:set_fm()`.

Example tempo-synced eighth-note LFO:

```lua
local lfo = synth.lfo("saw", 0.0)
lfo:set_sync("1/8")

function process(num_samples, sample_rate, tempo, playing, pos_beats)
    lfo:set_tempo(tempo or 120.0)
    local modulation = lfo:process(num_samples, sample_rate)
    -- Use modulation for a control-rate destination.
end
```

### Envelopes

`synth.adsr(attack_seconds, decay_seconds, sustain_level, release_seconds)` and
`synth.env(...)` create the same linear-segment ADSR envelope. Sustain is clamped
to `0..1`; times are non-negative seconds.

- `env:gate(true)` starts the attack on a rising gate.
- `env:gate(false)` starts the release on a falling gate.
- `env:trigger()` restarts from zero and starts the attack.
- `env:set_adsr(attack, decay, sustain, release)` updates parameters while active. A changed current stage restarts from its current level; sustain changes take effect immediately.
- `env:value()` reads the current level.
- `env:done()` is true after the release reaches zero and the envelope is idle.
- `env:process(num_samples, sample_rate)` advances the envelope over the block and returns its end level.

Use `osc:render_env()` or `osc:render_filtered()` for sample-by-sample amplitude
shaping without a Lua sample loop. `render_filtered()` advances the supplied ADSR
and LFO for every sample; the ADSR modulates the second filter stage and the LFO
modulates the first. If using
`env:process()` or `lfo:process()` for a custom destination, call it exactly once
for each block you intend to advance.

## `ui`

`draw()` rebuilds the script UI each frame (the editor requests a repaint every
50 ms). Widgets are commands submitted to the host's egui renderer; they do not
expose arbitrary drawing.

- `ui.heading(text)`
- `ui.label(text)`
- `ui.separator()` draws a separator oriented to the current layout.
- `ui.separator_horizontal()` always draws a horizontal line.
- `ui.separator_vertical()` always draws a vertical line.
- `ui.space(pixels?)`, defaulting to 8 logical pixels.
- `ui.button(id, label)` draws a button. A click currently produces only a temporary visual highlight; click state is not returned to Lua yet.
- `ui.param(index, label?)` draws a slider for macro `1..64`. Double-clicking its label resets the macro to its script default, or `0` if none was declared.
- `ui.radio_param(index, label, choices)` binds a radio group to macro `1..64`. It requires at least two string choices; the choices map evenly from normalized macro value `0` to `1`.
- `ui.meter(value, label?)` draws a progress bar clamped to `0..1`.
- `ui.row(function() ... end)` lays out child widgets horizontally.
- `ui.column(function() ... end)` lays out child widgets vertically.

Rows and columns must be properly nested. `ui.button()` IDs are used to track the
temporary click highlight, not as a Lua callback.

The same double-click reset works on labels in the editor's **All macros** list.
Use `ui.radio_param()` for discrete selections that must also be visible to the
audio/MIDI script state through `params.get()`; clicking a radio updates the bound
host macro parameter.

## `log`

`log(message)` writes a message to the process standard error stream with a `[lua]`
prefix. Availability of that output depends on how the host application was
started.

## Runtime Limits

The current implementation sets a 64 MiB Luau memory limit and instruction
budgets: 4,000,000 for loading, 250,000 per process/process-voice callback,
100,000 per MIDI event callback, and 250,000 per UI frame. Scripts run synchronously
on the plugin's processing or UI thread; the budgets do not make blocking or heavy
per-sample Lua loops real-time safe.
