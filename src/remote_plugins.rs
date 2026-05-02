use abi_stable::library::RootModule;
use anyhow::{Context, Result, anyhow};
use kkc_plugin_api::{KKC_REMOTE_PLUGIN_API_VERSION, RemotePluginModRef};
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct RemoteRustPluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub scheme: String,
    pub dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct RemoteRustPluginManifestInfo {
    pub id: String,
    pub name: String,
    pub dir: PathBuf,
}

#[derive(Debug, Deserialize)]
struct NativePluginManifest {
    plugin: NativePluginMetadata,
    remote: NativeRemoteMetadata,
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
struct NativeRemoteMetadata {
    library: String,
}

pub fn discover_remote_rust_plugins(plugins_dir: &Path) -> Result<Vec<RemoteRustPluginInfo>> {
    let manifests = discover_remote_rust_plugin_manifests(plugins_dir)?;
    let mut plugins = Vec::new();
    for manifest_info in manifests {
        let manifest = read_manifest(&manifest_info.dir.join("plugin.toml"))?;
        let Some(library_path) = resolve_remote_library_path(&manifest_info.dir, &manifest.remote.library)
        else {
            crate::viewer::debug_log(&format!(
                "startup: native remote plugin '{}' has no built library at {}",
                manifest.plugin.id,
                manifest_info.dir.join(&manifest.remote.library).display()
            ));
            continue;
        };
        let module = match RemotePluginModRef::load_from_file(&library_path) {
            Ok(module) => module,
            Err(err) => {
                crate::viewer::debug_log(&format!(
                    "startup: failed to load native remote plugin '{}': {err}",
                    manifest.plugin.id
                ));
                continue;
            }
        };
        let api_version = module.api_version()();
        if api_version != KKC_REMOTE_PLUGIN_API_VERSION {
            crate::viewer::debug_log(&format!(
                "Remote plugin '{}' uses API version {}, expected {}",
                manifest.plugin.id, api_version, KKC_REMOTE_PLUGIN_API_VERSION
            ));
            continue;
        }
        let metadata = module.metadata()();
        if metadata.id.as_str() != manifest.plugin.id {
            crate::viewer::debug_log(&format!(
                "Remote plugin '{}' exported id '{}'",
                manifest.plugin.id, metadata.id
            ));
            continue;
        }
        plugins.push(RemoteRustPluginInfo {
            id: metadata.id.to_string(),
            name: metadata.name.to_string(),
            version: metadata.version.to_string(),
            description: metadata.description.to_string(),
            scheme: metadata.scheme.to_string(),
            dir: manifest_info.dir,
        });
    }
    plugins.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(plugins)
}

pub fn discover_remote_rust_plugin_manifests(
    plugins_dir: &Path,
) -> Result<Vec<RemoteRustPluginManifestInfo>> {
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
        let manifest = read_manifest(&manifest_path)?;
        if manifest.plugin.plugin_type != "remote-rust" {
            continue;
        }
        plugins.push(RemoteRustPluginManifestInfo {
            id: manifest.plugin.id,
            name: manifest.plugin.name,
            dir: path,
        });
    }
    plugins.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(plugins)
}

#[allow(dead_code)]
pub fn load_remote_plugin(plugin_id: &str) -> Result<RemotePluginModRef> {
    let plugins_dir = crate::plugins::plugins_dir()?;
    for manifest_info in discover_remote_rust_plugin_manifests(&plugins_dir)? {
        if manifest_info.id == plugin_id {
            let manifest = read_manifest(&manifest_info.dir.join("plugin.toml"))?;
            let Some(library_path) = resolve_remote_library_path(&manifest_info.dir, &manifest.remote.library) else {
                return Err(not_found_error(
                    plugin_id,
                    &manifest_info.dir,
                    &manifest.remote.library,
                ));
            };
            return RemotePluginModRef::load_from_file(&library_path)
                .with_context(|| format!("Loading native remote plugin '{}'", plugin_id));
        }
    }
    Err(anyhow!(
        "Native remote plugin '{}' is not installed or built",
        plugin_id
    ))
}

fn resolve_remote_library_path(plugin_dir: &Path, configured_library: &str) -> Option<PathBuf> {
    let candidates = candidate_remote_library_paths(plugin_dir, configured_library);
    candidates.into_iter().find(|p| p.is_file())
}

fn candidate_remote_library_paths(plugin_dir: &Path, configured_library: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let configured = plugin_dir.join(configured_library);
    out.push(configured.clone());

    let Some(file_name) = Path::new(configured_library).file_name().map(|s| s.to_os_string()) else {
        return out;
    };
    for profile in ["release", "debug"] {
        out.push(plugin_dir.join("target").join(profile).join(&file_name));
    }

    if let Some(target_dir) = env::var_os("CARGO_TARGET_DIR") {
        let target_dir = PathBuf::from(target_dir);
        for profile in ["release", "debug"] {
            out.push(target_dir.join(profile).join(&file_name));
        }
    }

    if let Some(home) = env::var_os("HOME") {
        let home = PathBuf::from(home);
        for profile in ["release", "debug"] {
            out.push(home.join(".rust-target").join(profile).join(&file_name));
        }
    }

    out
}

fn not_found_error(plugin_id: &str, plugin_dir: &Path, configured_library: &str) -> anyhow::Error {
    let tried = candidate_remote_library_paths(plugin_dir, configured_library)
        .into_iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    anyhow!(
        "Native remote plugin '{}' is not installed or built (searched: {})",
        plugin_id, tried
    )
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
    if manifest.remote.library.trim().is_empty() {
        return Err(anyhow!(
            "{} contains an empty remote.library",
            path.display()
        ));
    }
    Ok(manifest)
}
