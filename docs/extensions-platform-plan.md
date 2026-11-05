# Extensions and Presets Platform Plan

**Status:** baseline proposal for future implementation. This document describes
the target platform; it does not claim that extension discovery, ZIP loading, or
preset browsing exists today.

## Goal

Move from editing a script embedded in a plugin instance to selecting a reusable
extension from the user's Documents folder. An extension may be a single Luau file
or a ZIP package containing modular Luau code and assets. Presets are separate JSON
files associated with an extension and creator. The longer-term goal is to let
extension authors build complete instruments and effects with a rich UI.

Keep CLAP/VST3 plugins as thin, fixed plugin types. The extension supplies behavior
and presentation; the plugin host continues to own audio/MIDI ports, macros,
automation, persistence, and the UI/audio thread boundary.

## Proposed User Data Layout

Use the platform's known Documents directory, not a hard-coded user profile path.
Keep the requested `extensions` and `presets` libraries directly under Documents;
reserve an application-specific subfolder for cache and temporary files:

```text
Documents/
  extensions/
    <extension-id>.lua
    <extension-id>.zip
    <extension-id>/                   # optional unpacked developer package
      manifest.json
      main.lua
      modules/
      assets/
  presets/
    <extension-id>/
      <creator-id>/
        <preset-id>.json
  Lua Plugin Host/
    cache/
      extensions/
        <extension-id>/<content-hash>/ # validated/extracted ZIP contents
```

Use stable IDs for directories and references; display names are mutable and may
not be unique. This keeps the proposed user-facing preset organization while
avoiding paths breaking when an extension is renamed. A UI may show
`<extension name>/<creator name>/<preset name>` while storing slugs/IDs on disk.

The directory names `extensions` and `presets` are the proposed baseline. The
cross-platform fallback behavior remains open. If the Documents known folder is
unavailable, report the fallback location in the UI and allow the user to open it.

## Extension Formats

### Single-file Luau extension

`extensions/<extension-id>.lua` is the low-friction format for a small extension.
It supplies the same callbacks as the current script API. A minimal metadata
sidecar may be added later; until then, synthesize a display name from the filename
and use a deterministic ID derived from it. Clearly mark missing creator/version
metadata in the browser.

### ZIP package

The ZIP contains one manifest, an entry script, optional modules, and optional
assets. Use the same internal layout for an unpacked development directory:

```text
manifest.json
main.lua
modules/
  voice.lua
  effects/filter.lua
assets/
  panel.png
  knob.svg
```

The package must not contain native libraries or executable plug-ins. Keep package
loading deterministic and make a package read-only to the runtime.

### Manifest baseline

Start with a small, versioned `manifest.json`:

```json
{
  "manifest_version": 1,
  "api_version": 1,
  "id": "com.example.warm-pad",
  "name": "Warm Pad",
  "version": "0.1.0",
  "creator": {
    "id": "example",
    "name": "Example Audio"
  },
  "description": "A polyphonic pad instrument.",
  "plugin_types": ["instrument"],
  "entry": "main.lua"
}
```

Baseline fields are stable extension ID, display name, semantic version, creator
ID/name, description, supported plugin types, and entry point. Add optional tags,
minimum host version, and UI metadata only when a concrete feature needs them.
Reject unsupported API versions and duplicate IDs with a visible diagnostic.

## Module Loading and Assets

- Implement extension-scoped `require()` for modules inside the package. Resolve
  module names only under `modules/` (or a documented package root), cache each
  module per Luau state, and never delegate to unrestricted system/package loading.
- Support `require("voice")` and nested names such as
  `require("effects/filter")`; define one canonical filename/extension convention.
- Add a read-only asset API that resolves paths under `assets/`. The UI host may
  decode/cache images, fonts, and other supported resources. Audio callbacks should
  not open files or decode assets.
- Version the asset API separately from the module loader only if compatibility
  requires it; initially include both under the extension `api_version`.

## Presets

Store presets as JSON under:

```text
Documents/presets/<extension-id>/<creator-id>/<preset-id>.json
```

Recommended baseline schema:

```json
{
  "preset_schema_version": 1,
  "extension_id": "com.example.warm-pad",
  "extension_version": "0.1.0",
  "creator_id": "example",
  "id": "wide-chorus",
  "name": "Wide Chorus",
  "parameters": {
    "macro.1": 0.72,
    "macro.2": 0.3
  },
  "state": {}
}
```

Presets should store parameter values and optional extension-defined JSON state;
they must not embed executable code or duplicate the extension package. Require
stable extension identity and schema versioning. Missing parameters use the
extension's declared reset defaults, then the host's zero default. Unknown fields
should be ignored for forward-compatible additions.

Preset loading must not silently switch the active extension. If a preset references
a missing or incompatible extension, report that clearly. Define migration hooks
only after a real schema change requires them.

## Host and Persistence Model

The current `ScriptState { name, source, path }` is embedded in plug-in state. The
target state should instead record the selected extension ID/version and preset ID,
alongside the existing host parameters. The extension source/package remains in the
user library and is never copied into each plug-in state.

Keep a migration path for existing inline scripts:

1. Continue loading legacy `ScriptState` instances.
2. Offer an explicit **Export as extension** action that writes a single `.lua`
   extension or a starter package.
3. After successful export/selection, persist its extension ID in the instance.
4. Do not discard the legacy source until the new extension loads successfully.

The exact serialization transition and rollback behavior should be designed before
changing `ScriptState`.

## Runtime and Security Requirements

- Discover/scan files and validate manifests away from the audio callback.
- Never read ZIPs, traverse user folders, compile modules, decode assets, or perform
  blocking I/O from `Plugin::process()`.
- Parse and validate a selected extension on the UI/background side, then prepare
  an immutable source snapshot for the audio and UI Luau states.
- Reload by extension ID/content version and keep the last working instance if a
  replacement fails to parse.
- Reject ZIP path traversal, absolute paths, symlinks, duplicate normalized paths,
  oversized entries, excessive file counts, and suspicious compression ratios.
- Restrict module and asset access to the selected package. Keep the existing Luau
  memory/instruction budgets and clearly communicate that scripts execute code in
  the host process.
- Keep audio and UI Lua state separate. Share only parameter/transport/asset handles
  that have explicit thread-safe ownership.

## Full UI Direction

The current UI API is command-based and immediate-mode. A fuller authoring API
should add, in stages:

1. Reliable button/action events returned to the script on the next UI frame.
2. Layout containers, groups/tabs, tooltips, and consistent spacing/alignment.
3. Controls such as knobs, switches, dropdowns, meters, and parameter binding.
4. Asset-backed images/icons and custom fonts with package-scoped loading.
5. A stable saved UI state mechanism for extension-defined state.

Keep UI construction on the UI thread. UI events should be queued safely to the
script state and must not call into audio-owned Lua objects.

## Implementation Phases

### Phase 1: Extension identity and discovery

- Define manifest/API versions, stable IDs, and Documents root resolution.
- Scan `.lua` files and package directories; show name, creator, type, and errors in
  an extension picker.
- Add a persisted selected-extension ID while retaining legacy inline scripts.
- Acceptance: selecting a single-file extension loads the same callbacks as today;
  restart restores the selection and the UI shows the selected extension.

### Phase 2: ZIP packages and modules

- Implement safe ZIP validation and content-hash extraction into the cache.
- Implement package-scoped `require()` with per-state module caching.
- Add unpacked-directory support for development using the exact same manifest.
- Acceptance: a ZIP with two Luau modules loads, an invalid/traversal ZIP is rejected,
  and no filesystem access occurs in processing callbacks.

### Phase 3: Preset library

- Implement JSON scan/save/load under the proposed extension/creator hierarchy.
- Store extension identity/version, macros, and a bounded JSON state object.
- Add browser actions for save, load, rename, duplicate, and delete.
- Acceptance: preset values restore after host restart, missing-extension presets
  remain visible as unavailable rather than silently loading another extension.

### Phase 4: Assets and UI expansion

- Add safe asset lookup and UI-thread decode/cache.
- Add image/font widgets and a proper button/event return path.
- Add controls and layouts suitable for complete synth/effect interfaces.
- Acceptance: an example packaged extension uses modules and assets in its UI while
  audio rendering remains independent of file I/O.

### Phase 5: Migration and distribution

- Add one-click export/migration for saved inline scripts.
- Define package signing/trust warnings, version compatibility, and update behavior.
- Document installation, backup, and recovery from broken extensions.

## Open Decisions

- Fallback locations when the platform Documents known folder is unavailable.
- Whether simple `.lua` files require a sidecar manifest or use filename-derived
  metadata until exported.
- Stable extension/creator ID format and duplicate-ID resolution.
- Exact module resolution rules and whether dependencies may be bundled or shared.
- Which asset formats to support initially and maximum decoded sizes.
- Whether extension-defined preset state is a plain JSON table or a restricted
  serialize/deserialize callback pair.
- Trust prompts, extension enable/disable controls, and whether extensions can be
  loaded from outside the Documents library.
- Which widgets and event model are required for the first full synth UI.
