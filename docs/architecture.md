# Architecture

## Workspace

The workspace has one shared core and three thin plugin crates:

- `crates/core`: parameters, script persistence, Luau hosting, audio/MIDI bridges,
  voice allocation, synthesis helpers, and the egui UI bridge.
- `crates/instrument`: MIDI-in/audio-out instrument.
- `crates/audio-fx`: audio-in/audio-out effect with an auxiliary sidechain input.
- `crates/midi-fx`: MIDI-in/MIDI-out effect.

Each plugin exports both CLAP and VST3. The plugins have separate identities and
port layouts so hosts can categorize and route them correctly.

## Script Lifecycle

Scripts are stored in the plugin's persisted `ScriptState`. **Apply & reload**
updates the source and increments a version counter. The audio and UI hosts notice
that version and create fresh Luau states from the same source. The UI host also
compares the persisted script name and source, so a state restored without a version
counter change still refreshes the editor script.

There are two independent Luau states per plugin instance:

- The audio state runs DSP, MIDI, and voice callbacks synchronously in the plugin
  process path.
- The UI state runs `draw()` and owns its own Lua locals.

The states share plugin parameters and the output meter, but not Lua tables or local
variables. A table changed in `process_voice()` is not visible to `draw()`.
Reloading recreates both states, so Lua-local oscillator and envelope state resets.

## Callback Flow

### Instrument

1. MIDI events update the host's note/voice allocator.
2. `on_midi(event, a, b, c, d)` is called for supported MIDI events, if defined.
3. If the script defines `process_voice(voice)`, it is called once for each active
   voice. That callback takes precedence over `process()` for instrument audio.
4. Return `true` from `process_voice()` to finish a voice and release its resources.
   `voice_ended(id)` is called when a voice is removed, including voice stealing.
5. If there is no `process_voice()`, an instrument may use the general `process()`
   callback instead.

Mono mode uses last-note priority: releasing the newest held note resumes the
previously held note. Poly mode allocates up to **Max Voices**; when full, it
prefers stealing a released voice, then steals the oldest voice.

### Audio FX

`process(num_samples, sample_rate, tempo, playing, pos_beats)` runs once per audio
block. The main audio buffer contains the incoming audio and can be read or changed
through `audio.get()`, `audio.set()`, or `audio.gain()`.

### MIDI FX

`on_midi()` processes incoming MIDI events. Then `process()` runs once per block,
which allows transport-aware event generation such as arpeggiators. Events queued
with `midi.send_*()` are sent to the host at the end of the block. Incoming events
are not automatically passed through; scripts must send the events they want to
preserve.

## Parameters and State

Every plugin has 64 normalized macro parameters, read in Lua with
`params.get(index)`. The shared parameter set also contains automatable **Voice
Mode** and **Max Voices** controls (Poly and 8 by default); only the instrument
uses them, and its custom editor displays them. Lua may override these with
`synth.set_mode()` and `synth.set_max_voices()`; overrides are reset when the
script is loaded and can be cleared with `"default"` or `nil`.

Scripts can declare per-script macro reset values with `params.set_default()` or
`params.set_defaults()`. Applying a script preserves current macro values; the
editor's **Reset macros to script defaults** button explicitly applies the
declarations. These are script reset values, not changes to the DAW's native
parameter default metadata.

The custom editor exposes voice controls for the instrument, all macros, the Lua
script editor, and script-defined UI commands. The script text and its name are
persisted with the plugin state.

## Runtime Constraints

Luau runs synchronously in the plugin process path. The runtime applies a 64 MiB
memory limit and instruction budgets for script loading, processing, MIDI, and UI
callbacks. These limits are safety bounds, not real-time isolation: expensive Lua
loops, allocations, or blocking work can still affect audio performance.

Prefer Rust-backed helpers such as `synth.osc("saw"):render(...)`,
`osc:render_env()`, and `osc:render_filtered()` for per-sample synthesis, envelope,
and filter work. LFO and envelope objects can advance in Rust for each sample
rendered. See the [Lua API reference](lua-api.md) for exact semantics and limits.
