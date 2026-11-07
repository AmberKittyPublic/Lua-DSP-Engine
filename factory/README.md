# Factory Scripts

These scripts are starting points for the three plugin types. The plugin copies
them into `Documents/Lumina DSP/extensions/{synth,audio-fx,midi-fx}/` on first open; choose
**Load script...** in the plugin's **Script** panel to open that folder and load
one. To apply the values declared in a script's `params.set_defaults()` call,
choose **Reset macros to script defaults**; loading a script otherwise preserves
existing macro values.

- `midi-fx/step-sequencer.lua`: 12-pitch by 4/8/16/32-step note grid where each
  cell toggles a chromatic pitch; empty steps use the held chord (ARP) in live
  mode or stay silent in MIDI-trigger mode; 17 synced rates; eight chord
  note-walk modes; ratchets, swing, and humanization.
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
- `extensions/acid-303/`: mono 303-style synth with filter-envelope modulation and
  selectable saturator, bitcrush, tube, tape, overdrive, fuzz, and diode distortion.
  Its ADSR envelope can add up to 24 dB of distortion drive per note.
- `audio-fx/stereo-delay.lua`: stereo delay with Digital, Tape, Analog, BBD, and
  Lo-fi characters; free or tempo-synced timing; ping-pong routing; feedback tone,
  modulation, width, drive, and a repeat-pattern display.
- `audio-fx/stereo-compressor.lua`: linked broadband stereo compressor with Clean,
  Glue, Transistor, Tube, and Optical characteristics, an external sidechain blend,
  and soft-knee controls.
- `audio-fx/three-band-compressor.lua`: linked three-band compressor with adjustable
  crossovers, independent threshold, ratio, attack, release, knee, and makeup per
  band, the same five Character modes, and a band-split external sidechain.
- `audio-fx/stereo-reverb.lua`: Room, Hall, Plate, Chamber, and Spring spaces with
  adjustable decay, damping, predelay, size, diffusion, modulation, early
  reflections, stereo width, freeze, and a tail-profile display.
- `audio-fx/distortion-suite.lua`: selectable saturator, bitcrush/downsample, tube,
  tape, overdrive, fuzz, and diode modes, with shared drive, tone, bias, and mix
  controls. Bit depth and sample-rate reduction are adjustable in digital mode.

All scripts use the shared 64 macro parameters; each script documents its macro
assignments in its header.

See [`docs/lua-api.md`](../docs/lua-api.md) for the complete callback and API
reference.

The shared API also provides `synth.lfo(shape, hz)` and `synth.adsr(a, d, s, r)`.
Their `:process(num_samples, sample_rate)` methods advance in Rust over the block,
so scripts can add control-rate modulation and envelopes without per-sample Lua
loops.
