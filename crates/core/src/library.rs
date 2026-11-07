use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::params::PluginKind;
use serde::{Deserialize, Serialize};

pub const LIBRARY_FOLDER: &str = "Lumina DSP";
pub const SCRIPTS_FOLDER: &str = "scripts";
pub const EXTENSIONS_FOLDER: &str = "extensions";
pub const PRESETS_FOLDER: &str = "presets";
pub const CACHE_FOLDER: &str = "cache";
const MAX_SCRIPT_BYTES: u64 = 4 * 1024 * 1024;

struct FactoryScript {
    kind: PluginKind,
    name: &'static str,
    source: &'static str,
}

const FACTORY_SCRIPTS: &[FactoryScript] = &[
    FactoryScript {
        kind: PluginKind::Instrument,
        name: "808-kick.lua",
        source: include_str!("../../../factory/instrument/808-kick.lua"),
    },
    FactoryScript {
        kind: PluginKind::Instrument,
        name: "clap.lua",
        source: include_str!("../../../factory/instrument/clap.lua"),
    },
    FactoryScript {
        kind: PluginKind::Instrument,
        name: "closed-hihat.lua",
        source: include_str!("../../../factory/instrument/closed-hihat.lua"),
    },
    FactoryScript {
        kind: PluginKind::Instrument,
        name: "fm-donk.lua",
        source: include_str!("../../../factory/instrument/fm-donk.lua"),
    },
    FactoryScript {
        kind: PluginKind::Instrument,
        name: "gated-sine-pad.lua",
        source: include_str!("../../../factory/instrument/gated-sine-pad.lua"),
    },
    FactoryScript {
        kind: PluginKind::Instrument,
        name: "kick.lua",
        source: include_str!("../../../factory/instrument/kick.lua"),
    },
    FactoryScript {
        kind: PluginKind::Instrument,
        name: "open-hihat.lua",
        source: include_str!("../../../factory/instrument/open-hihat.lua"),
    },
    FactoryScript {
        kind: PluginKind::Instrument,
        name: "sine-pad.lua",
        source: include_str!("../../../factory/instrument/sine-pad.lua"),
    },
    FactoryScript {
        kind: PluginKind::Instrument,
        name: "snare.lua",
        source: include_str!("../../../factory/instrument/snare.lua"),
    },
    FactoryScript {
        kind: PluginKind::AudioFx,
        name: "stereo-compressor.lua",
        source: include_str!("../../../factory/audio-fx/stereo-compressor.lua"),
    },
    FactoryScript {
        kind: PluginKind::AudioFx,
        name: "three-band-compressor.lua",
        source: include_str!("../../../factory/audio-fx/three-band-compressor.lua"),
    },
    FactoryScript {
        kind: PluginKind::AudioFx,
        name: "stereo-delay.lua",
        source: include_str!("../../../factory/audio-fx/stereo-delay.lua"),
    },
    FactoryScript {
        kind: PluginKind::AudioFx,
        name: "stereo-reverb.lua",
        source: include_str!("../../../factory/audio-fx/stereo-reverb.lua"),
    },
    FactoryScript {
        kind: PluginKind::AudioFx,
        name: "distortion-suite.lua",
        source: include_str!("../../../factory/audio-fx/distortion-suite.lua"),
    },
    FactoryScript {
        kind: PluginKind::MidiFx,
        name: "step-sequencer.lua",
        source: include_str!("../../../factory/midi-fx/step-sequencer.lua"),
    },
    FactoryScript {
        kind: PluginKind::MidiFx,
        name: "transpose.lua",
        source: include_str!("../../../factory/midi-fx/transpose.lua"),
    },
];

struct FactoryExtension {
    kind: PluginKind,
    name: &'static str,
    files: &'static [(&'static str, &'static str)],
}

const FACTORY_EXTENSIONS: &[FactoryExtension] = &[FactoryExtension {
    kind: PluginKind::Instrument,
    name: "acid-303",
    files: &[
        (
            "manifest.json",
            include_str!("../../../factory/extensions/acid-303/manifest.json"),
        ),
        (
            "main.lua",
            include_str!("../../../factory/extensions/acid-303/main.lua"),
        ),
        (
            "modules/voice.lua",
            include_str!("../../../factory/extensions/acid-303/modules/voice.lua"),
        ),
        (
            "modules/ui.lua",
            include_str!("../../../factory/extensions/acid-303/modules/ui.lua"),
        ),
    ],
}];

pub fn library_root() -> Option<PathBuf> {
    dirs::document_dir().map(|documents| documents.join(LIBRARY_FOLDER))
}

pub fn script_dir(kind: PluginKind) -> Option<PathBuf> {
    library_root().map(|root| root.join(SCRIPTS_FOLDER).join(kind.folder_name()))
}

pub fn scripts_dir(kind: PluginKind) -> Option<PathBuf> {
    script_dir(kind)
}

pub fn extensions_dir(kind: PluginKind) -> Option<PathBuf> {
    library_root().map(|root| {
        root.join(EXTENSIONS_FOLDER)
            .join(kind.extension_folder_name())
    })
}

pub fn extension_dir(kind: PluginKind) -> Option<PathBuf> {
    extensions_dir(kind)
}

pub fn presets_dir() -> Option<PathBuf> {
    library_root().map(|root| root.join(PRESETS_FOLDER))
}

pub fn cache_dir() -> Option<PathBuf> {
    library_root().map(|root| root.join(CACHE_FOLDER))
}

pub fn extension_cache_dir() -> Option<PathBuf> {
    cache_dir().map(|root| root.join(EXTENSIONS_FOLDER))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionCreator {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionManifest {
    pub manifest_version: u32,
    pub api_version: u32,
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub version: String,
    #[serde(default)]
    pub creator: Option<ExtensionCreator>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub plugin_types: Option<Vec<String>>,
    pub entry: String,
}

#[derive(Debug, Clone)]
pub struct ExtensionPackage {
    pub root: PathBuf,
    pub manifest_path: PathBuf,
    pub manifest: ExtensionManifest,
    pub entry_source: String,
    pub modules: BTreeMap<String, String>,
}

pub fn read_extension(path: &Path) -> Result<ExtensionPackage, String> {
    let manifest_path = if path.is_dir() {
        path.join("manifest.json")
    } else if path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.eq_ignore_ascii_case("manifest.json"))
        .unwrap_or(false)
    {
        path.to_path_buf()
    } else {
        return Err(format!(
            "{} is not an extension manifest or directory",
            path.display()
        ));
    };

    if !manifest_path.is_file() {
        return Err(format!(
            "extension manifest not found: {}",
            manifest_path.display()
        ));
    }

    let manifest_text = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("could not read {}: {error}", manifest_path.display()))?;
    let manifest: ExtensionManifest = serde_json::from_str(&manifest_text).map_err(|error| {
        format!(
            "invalid extension manifest {}: {error}",
            manifest_path.display()
        )
    })?;

    if manifest.manifest_version != 1 {
        return Err(format!(
            "unsupported extension manifest version: {}",
            manifest.manifest_version
        ));
    }
    if manifest.api_version != 1 {
        return Err(format!(
            "unsupported extension API version: {}",
            manifest.api_version
        ));
    }

    let root = manifest_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let entry_path = root.join(&manifest.entry);
    if !entry_path.is_file() {
        return Err(format!(
            "extension entry not found: {}",
            entry_path.display()
        ));
    }
    let entry_source = fs::read_to_string(&entry_path)
        .map_err(|error| format!("could not read {}: {error}", entry_path.display()))?;

    let mut modules = BTreeMap::new();
    let modules_dir = root.join("modules");
    if modules_dir.is_dir() {
        collect_modules(&modules_dir, &modules_dir, "", &mut modules)?;
    }

    Ok(ExtensionPackage {
        root,
        manifest_path,
        manifest,
        entry_source,
        modules,
    })
}

fn collect_modules(
    root: &Path,
    dir: &Path,
    prefix: &str,
    modules: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    for entry in
        fs::read_dir(dir).map_err(|error| format!("could not read {}: {error}", dir.display()))?
    {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let next_prefix = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            collect_modules(root, &path, &next_prefix, modules)?;
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("lua"))
            .unwrap_or(false)
        {
            let stem = path
                .file_stem()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let key = if prefix.is_empty() {
                stem
            } else {
                format!("{prefix}/{stem}")
            };
            let source = fs::read_to_string(&path)
                .map_err(|error| format!("could not read {}: {error}", path.display()))?;
            modules.insert(key, source);
        }
    }
    let _ = root;
    Ok(())
}

pub fn install_factory_library() -> Result<PathBuf, String> {
    let root = library_root().ok_or_else(|| {
        "could not locate the Documents folder for the script library".to_string()
    })?;
    ensure_structure(&root)
        .map_err(|error| format!("could not create the script library: {error}"))?;
    install_factory_scripts(&root)
        .map_err(|error| format!("could not install the factory scripts: {error}"))?;
    install_factory_extensions(&root)
        .map_err(|error| format!("could not install the factory extensions: {error}"))?;
    Ok(root)
}

pub fn read_script(path: &Path) -> Result<String, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    if metadata.len() > MAX_SCRIPT_BYTES {
        return Err(format!(
            "{} is larger than the {} MiB script limit",
            path.display(),
            MAX_SCRIPT_BYTES / (1024 * 1024)
        ));
    }
    fs::read_to_string(path).map_err(|error| format!("could not read {}: {error}", path.display()))
}

fn ensure_structure(root: &Path) -> io::Result<()> {
    fs::create_dir_all(root.join(PRESETS_FOLDER))?;
    fs::create_dir_all(root.join(CACHE_FOLDER).join(EXTENSIONS_FOLDER))?;
    for kind in [
        PluginKind::Instrument,
        PluginKind::AudioFx,
        PluginKind::MidiFx,
    ] {
        fs::create_dir_all(
            root.join(EXTENSIONS_FOLDER)
                .join(kind.extension_folder_name()),
        )?;
    }
    Ok(())
}

fn install_factory_scripts(root: &Path) -> io::Result<()> {
    ensure_structure(root)?;
    for script in FACTORY_SCRIPTS {
        let path = root
            .join(EXTENSIONS_FOLDER)
            .join(script.kind.extension_folder_name())
            .join(script.name);
        if !path.exists() {
            fs::write(path, script.source)?;
        }
    }
    Ok(())
}

fn install_factory_extensions(root: &Path) -> io::Result<()> {
    ensure_structure(root)?;
    for extension in FACTORY_EXTENSIONS {
        let extension_root = root
            .join(EXTENSIONS_FOLDER)
            .join(extension.kind.extension_folder_name())
            .join(extension.name);
        for (relative_path, source) in extension.files {
            let path = extension_root.join(relative_path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            if !path.exists() {
                fs::write(path, source)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn factory_scripts_cover_every_plugin_kind() {
        for kind in [
            PluginKind::Instrument,
            PluginKind::AudioFx,
            PluginKind::MidiFx,
        ] {
            assert!(
                FACTORY_SCRIPTS
                    .iter()
                    .any(|script| script.kind == kind && !script.source.is_empty()),
                "{kind:?} has no factory scripts"
            );
        }
    }

    #[test]
    fn acid_extension_manifest_and_modules_load() {
        let root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../factory/extensions/acid-303");
        let package = read_extension(&root.join("manifest.json")).expect("extension should load");
        assert_eq!(package.manifest.id, "com.daydrm.acid-303");
        assert!(package.modules.contains_key("voice"));
        assert!(package.modules.contains_key("ui"));
    }

    #[test]
    fn installation_keeps_existing_scripts_and_fills_missing_ones() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after the epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "lua-plugin-host-library-test-{}-{unique}",
            std::process::id()
        ));

        install_factory_scripts(&root).expect("initial installation should succeed");
        let first = root.join(EXTENSIONS_FOLDER).join("synth").join("kick.lua");
        assert!(first.exists());
        fs::write(&first, "-- user edit").expect("test edit should succeed");

        install_factory_scripts(&root).expect("repeat installation should succeed");
        assert_eq!(
            fs::read_to_string(&first).expect("script should remain readable"),
            "-- user edit"
        );
        assert!(
            root.join(PRESETS_FOLDER).is_dir(),
            "preset folder should be initialized"
        );

        fs::remove_dir_all(root).expect("temporary library should be removable");
    }
}
