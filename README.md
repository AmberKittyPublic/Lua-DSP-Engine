# Lua Plugin Host

A Luau-powered audio plugin workspace with three CLAP/VST3 plugins:

- **Lua Instrument**: MIDI input to mono/polyphonic audio output.
- **Lua Audio FX**: audio processing with an optional stereo or mono sidechain.
- **Lua MIDI FX**: MIDI input/output transformations and transport-aware scripts.

Each plugin shares the Lua runtime, 64 automatable macros, script library, and UI
API from `lua-plugin-core`. Load scripts from the plugin window with the OS file
picker, and the selected source is persisted with the plugin state. On first open,
the plugin creates `Documents/Lua Plugin Host/` with per-type script folders and a
presets folder, then installs the bundled factory scripts into extensions/ without overwriting
existing files.

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
- [Agent and contributor context](agents.md): project conventions and the
  requirement to keep guidance and docs updated alongside changes.

## Quick Start

Run the default workspace checks and build:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --release
```

The plugin editor contains a **Script** panel. **Load script...** opens the OS file
picker in the matching `Documents/Lua Plugin Host/scripts/` folder;
**Reset to default** restores the built-in example script. Existing instances keep
their saved script source until another script is loaded or defaults are reset;
changing a library file does not update instances that already loaded it. Scripts
may declare macro reset values; use **Reset macros to script defaults** when you
want those values applied without changing the DAW's native defaults.

For cross-compilation, multi-platform builds, and generated bundles, see [Building and testing](docs/building.md). Run `./build-all.sh` to build Linux and Windows locally; macOS is built when its toolchain is available.
