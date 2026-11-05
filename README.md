# Lua Plugin Host

A Luau-powered audio plugin workspace with three CLAP/VST3 plugins:

- **Lua Instrument**: MIDI input to mono/polyphonic audio output.
- **Lua Audio FX**: audio processing with an optional stereo or mono sidechain.
- **Lua MIDI FX**: MIDI input/output transformations and transport-aware scripts.

Each plugin shares the Lua runtime, 64 automatable macros, script editor, and UI
API from `lua-plugin-core`. Scripts are edited in the plugin window and persisted
with the plugin state. To use a factory example, paste its contents into the
script editor and press **Apply & reload**.

## Documentation

- [Lua API reference](docs/lua-api.md): callbacks, globals, audio/MIDI/UI APIs,
  voices, oscillators, LFOs, and envelopes.
- [Architecture](docs/architecture.md): plugin types, callback lifecycle, voice
  handling, and runtime constraints.
- [Building and testing](docs/building.md): development commands and Windows
  cross-build artifacts.
- [Extensions platform plan](docs/extensions-platform-plan.md): proposed extension
  folders, ZIP modules/assets, presets, migration, and implementation phases.
- [Factory scripts](factory/README.md): included MIDI effects, instruments, and audio effects.
- [Agent and contributor context](context/agents.md): project conventions and the
  requirement to keep guidance and docs updated alongside changes.

## Quick Start

Run the default workspace checks and build:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --release
```

The plugin editor contains a **Script** panel with an editable Luau source field.
After editing, use **Apply & reload**. **Reset to default** restores the built-in
example script. Existing instances keep their saved script until reset or edited;
changes to files under `factory/` do not automatically update plugin instances.
Scripts may declare macro reset values; use **Reset macros to script defaults**
when you want those values applied without changing the DAW's native defaults.

For cross-compilation and generated bundles, see [Building and testing](docs/building.md).
