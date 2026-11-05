# Factory Scripts

These scripts are starting points for the three plugin types. The plugin copies
them into `Documents/Lua Plugin Host/extensions/{synth,audio-fx,midi-fx}/` on first open; choose
**Load script...** in the plugin's **Script** panel to open that folder and load
one. To apply the values declared in a script's `params.set_defaults()` call,
choose **Reset macros to script defaults**; loading a script otherwise preserves
existing macro values.

- `midi-fx/16th-arpeggiator.lua`: transport-synced sixteenth-note arp. Hold MIDI
  notes and start playback. Macros control gate, octave span, and direction.
- `midi-fx/transpose.lua`: octave selection with a macro-bound radio group plus a
  semitone slider. Change `OCTAVE_RANGE` in the script to change its octave span.
- `instrument/fm-donk.lua`: polyphonic FM bass with a short pitch drop and ADSR
  envelope and an integer 1..16 unison control. The instrument's Voice Mode and
  Max Voices controls apply.
- `instrument/sine-pad.lua`: polyphonic pad with ADSR controls arranged in two
  rows.
- `instrument/gated-sine-pad.lua`: sine body plus a filtered saw overtone; an
  inverted, tempo-synced 1/8 LFO and ADSR sweep separate filter stages. LFO depth
  (0..6 octaves) and ADSR filter depth (0..4 octaves) are independent; zero disables
  that filter modulation. Its unison count uses an integer 1..16 macro scale, with
  detune and stereo spread alongside it.
- `instrument/kick.lua`: low sine/FM kick with a sample-smooth pitch drop and a
  filtered white-noise layer shaped by the same ADSR.
- `instrument/clap.lua`: wide clap layered from pink and white noise.
- `instrument/open-hihat.lua` and `instrument/closed-hihat.lua`: filtered white-noise
  hats with longer and shorter envelopes, respectively.
- `instrument/snare.lua`: tuned FM body layered with filtered pink-noise wires.
- `audio-fx/stereo-delay.lua`: stereo feedback delay with optional ping-pong
  routing and subtle LFO modulation.
- `audio-fx/stereo-compressor.lua`: linked stereo compressor with an external
  sidechain blend and soft-knee controls. Its extension computes the live level,
  ducking, and transfer-curve series for the generic graph widget.
- `audio-fx/stereo-reverb.lua`: stereo comb/allpass reverb with room, damping, wet,
  and width controls.
- `audio-fx/saturator.lua`: soft-clipping saturator with drive, tone, output, and
  dry/wet mix controls. Its extension computes and plots the current transfer curve.

All scripts use the shared 64 macro parameters; each script documents its macro
assignments in its header.

See [`docs/lua-api.md`](../docs/lua-api.md) for the complete callback and API
reference.

The shared API also provides `synth.lfo(shape, hz)` and `synth.adsr(a, d, s, r)`.
Their `:process(num_samples, sample_rate)` methods advance in Rust over the block,
so scripts can add control-rate modulation and envelopes without per-sample Lua
loops.
