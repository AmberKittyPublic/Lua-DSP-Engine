# Building and Testing

## Requirements

- Rust toolchain matching the workspace's `rust-version` (1.88 or newer).
- Platform build dependencies required by `nice-plug-egui` and its windowing/OpenGL
  dependencies.
- To cross-build Windows GNU binaries from Linux, an x86-64 MinGW-w64 toolchain and
  the `x86_64-pc-windows-gnu` Rust target.

## Development

From the workspace root:

```sh
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
```

The workspace contains `lua-plugin-core`, `lua-plugin-instrument`,
`lua-plugin-audio-fx`, and `lua-plugin-midi-fx`. Core tests cover voice allocation,
Lua APIs, factory-script loading, and the MIDI arpeggiator's generated note events.

## Documentation Maintenance

Keep the user-facing docs and `context/agents.md` current as implementation changes
are made. Update them in the same work as changes to public APIs, callbacks,
parameters, plugin behavior, factory examples, build commands, or project
conventions. See [Agent and Contributor Context](../context/agents.md) for the
project-specific workflow.

## Windows GNU Cross-Build

The configured cross-build command is:

```sh
cargo build --workspace --release --target x86_64-pc-windows-gnu
```

The current `.cargo/config.toml` assumes the MinGW C++ static library is at
`/usr/x86_64-w64-mingw32/sys-root/mingw/lib`, as on the build environment used for
these artifacts. Adjust that target-specific search path for other Linux
distributions/toolchain layouts. The C++ runtime is statically linked in these
Windows binaries; standard Windows system DLLs remain external dependencies.

The current packaged outputs are:

- `dist/windows/clap/*.clap`
- `dist/windows/vst3/*.vst3/Contents/x86_64-win/*.vst3`
- `dist/windows/raw/*.dll`
- `dist/lua-plugin-host-windows-x86_64.zip`

The bundles are generated artifacts and are not automatically installed into a
DAW's plugin scan folders. Copy the desired CLAP file or complete VST3 bundle to a
plugin folder recognized by the target host, rescan plugins, and restart/reload the
plugin instance as needed.

## Other Targets

For a non-Windows target, use Cargo's corresponding target triple and install the
target with `rustup target add <target>`. The Windows GNU linker configuration is
target-specific, but the absolute MinGW library path applies only to that Linux
cross-build setup. This workspace does not currently define a packaging command
for macOS or other targets.
