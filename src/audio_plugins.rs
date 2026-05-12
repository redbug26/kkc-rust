use abi_stable::library::lib_header_from_path;
use anyhow::{Context, Result, anyhow};
use kkc_plugin_api::{AudioPluginModRef, KKC_AUDIO_PLUGIN_API_VERSION};
use serde::Deserialize;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AudioRustPluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub mime_types: Vec<String>,
    pub extensions: Vec<String>,
    pub dir: PathBuf,
}

#[derive(Debug, Deserialize)]
struct NativePluginManifest {
    plugin: NativePluginMetadata,
    audio: Option<NativeAudioMetadata>,
}

#[derive(Debug, Deserialize)]
struct NativePluginMetadata {
    id: String,
    name: String,
    version: String,
    description: String,
    #[serde(rename = "type")]
    plugin_type: String,
}

#[derive(Debug, Deserialize)]
struct NativeAudioMetadata {
    #[serde(default)]
    mime_types: Vec<String>,
    library: String,
}

pub fn discover_audio_rust_plugins(plugins_dir: &Path) -> Result<Vec<AudioRustPluginInfo>> {
    let mut plugins = Vec::new();
    for entry in fs::read_dir(plugins_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest_path = path.join("plugin.toml");
        if !manifest_path.is_file() {
            continue;
        }
        let manifest = match read_manifest(&manifest_path) {
            Ok(manifest) => manifest,
            Err(err) => {
                crate::viewer::debug_log(&format!(
                    "startup: native audio plugin manifest discovery failed: {err}"
                ));
                continue;
            }
        };
        if manifest.plugin.plugin_type != "audio-rust" {
            continue;
        }

        let Some(audio) = manifest.audio.as_ref() else {
            crate::viewer::debug_log(&format!(
                "startup: native audio plugin '{}' missing [audio] section",
                manifest.plugin.id
            ));
            continue;
        };

        let Some(library_path) = resolve_audio_library_path(&path, &audio.library) else {
            crate::viewer::debug_log(&format!(
                "startup: native audio plugin '{}' has no built library",
                manifest.plugin.id
            ));
            continue;
        };

        let module = match lib_header_from_path(&library_path)
            .and_then(|h| h.init_root_module::<AudioPluginModRef>())
        {
            Ok(module) => module,
            Err(err) => {
                crate::viewer::debug_log(&format!(
                    "startup: failed to load native audio plugin '{}': {err}",
                    manifest.plugin.id
                ));
                continue;
            }
        };

        let api_version = module.api_version()();
        if api_version != KKC_AUDIO_PLUGIN_API_VERSION {
            crate::viewer::debug_log(&format!(
                "Audio plugin '{}' uses API version {}, expected {}",
                manifest.plugin.id, api_version, KKC_AUDIO_PLUGIN_API_VERSION
            ));
            continue;
        }

        let metadata = module.metadata()();
        if metadata.id.as_str() != manifest.plugin.id {
            crate::viewer::debug_log(&format!(
                "Audio plugin '{}' exported id '{}'",
                manifest.plugin.id, metadata.id
            ));
            continue;
        }

        let mut mime_types = if metadata.mime_types.is_empty() {
            audio.mime_types.clone()
        } else {
            metadata
                .mime_types
                .iter()
                .map(|value| value.as_str().trim().to_ascii_lowercase())
                .collect::<Vec<_>>()
        };
        mime_types.retain(|value| !value.is_empty());
        mime_types.sort();
        mime_types.dedup();

        let mut extensions = metadata
            .extensions
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .trim()
                    .trim_start_matches('.')
                    .to_ascii_lowercase()
            })
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        extensions.sort();
        extensions.dedup();

        plugins.push(AudioRustPluginInfo {
            id: metadata.id.to_string(),
            name: metadata.name.to_string(),
            version: metadata.version.to_string(),
            description: metadata.description.to_string(),
            mime_types,
            extensions,
            dir: path,
        });
    }

    plugins.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(plugins)
}

pub fn load_audio_plugin(plugin_id: &str) -> Result<AudioPluginModRef> {
    let plugins_dir = crate::plugins::plugins_dir()?;
    for entry in fs::read_dir(&plugins_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest_path = path.join("plugin.toml");
        if !manifest_path.is_file() {
            continue;
        }
        let manifest = match read_manifest(&manifest_path) {
            Ok(manifest) => manifest,
            Err(err) => {
                crate::viewer::debug_log(&format!(
                    "startup: native audio plugin discovery failed: {err}"
                ));
                continue;
            }
        };
        if manifest.plugin.plugin_type != "audio-rust" || manifest.plugin.id != plugin_id {
            continue;
        }

        let Some(audio) = manifest.audio.as_ref() else {
            return Err(anyhow!(
                "Native audio plugin '{}' missing [audio]",
                plugin_id
            ));
        };

        let Some(library_path) = resolve_audio_library_path(&path, &audio.library) else {
            return Err(anyhow!(
                "Native audio plugin '{}' is not installed or built",
                plugin_id
            ));
        };

        return lib_header_from_path(&library_path)
            .and_then(|h| h.init_root_module::<AudioPluginModRef>())
            .with_context(|| format!("Loading native audio plugin '{}'", plugin_id));
    }

    Err(anyhow!(
        "Native audio plugin '{}' is not installed or built",
        plugin_id
    ))
}

fn resolve_audio_library_path(plugin_dir: &Path, configured_library: &str) -> Option<PathBuf> {
    let candidates = candidate_audio_library_paths(plugin_dir, configured_library);
    candidates.into_iter().find(|p| p.is_file())
}

fn candidate_audio_library_paths(plugin_dir: &Path, configured_library: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let configured_rel = Path::new(configured_library);
    let configured = plugin_dir.join(configured_rel);
    out.push(configured.clone());

    let parent = configured_rel.parent().unwrap_or_else(|| Path::new(""));
    let file_names = library_file_name_variants(configured_rel);
    for file_name in &file_names {
        out.push(plugin_dir.join(parent).join(file_name));
    }

    let Some(file_name) = file_names.first() else {
        return out;
    };

    out.push(plugin_dir.join(file_name));

    for profile in ["release", "debug"] {
        out.push(plugin_dir.join("target").join(profile).join(file_name));
    }

    if let Some(target_dir) = env::var_os("CARGO_TARGET_DIR") {
        let target_dir = PathBuf::from(target_dir);
        for profile in ["release", "debug"] {
            out.push(target_dir.join(profile).join(file_name));
        }
    }

    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        for profile in ["release", "debug"] {
            out.push(home.join(".rust-target").join(profile).join(file_name));
        }
    }

    out.sort();
    out.dedup();
    out
}

fn library_file_name_variants(configured_library: &Path) -> Vec<OsString> {
    let Some(file_name) = configured_library.file_name() else {
        return Vec::new();
    };

    let mut out = vec![file_name.to_os_string()];
    let stem = configured_library
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if stem.is_empty() {
        return out;
    }

    let native_ext = std::env::consts::DLL_EXTENSION;
    for ext in [native_ext, "dylib", "so", "dll"] {
        let candidate = OsString::from(format!("{stem}.{ext}"));
        if !out.iter().any(|existing| existing == &candidate) {
            out.push(candidate);
        }
    }
    out
}

fn read_manifest(path: &Path) -> Result<NativePluginManifest> {
    let text = fs::read_to_string(path).with_context(|| format!("Reading {}", path.display()))?;
    let manifest: NativePluginManifest =
        toml::from_str(&text).with_context(|| format!("Parsing {}", path.display()))?;

    if manifest.plugin.id.trim().is_empty()
        || manifest.plugin.name.trim().is_empty()
        || manifest.plugin.version.trim().is_empty()
        || manifest.plugin.description.trim().is_empty()
    {
        return Err(anyhow!("{} contains empty plugin metadata", path.display()));
    }

    if manifest.plugin.plugin_type == "audio-rust"
        && manifest
            .audio
            .as_ref()
            .map(|audio| audio.library.trim().is_empty())
            .unwrap_or(true)
    {
        return Err(anyhow!("{} contains empty audio.library", path.display()));
    }

    Ok(manifest)
}
