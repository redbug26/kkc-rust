use abi_stable::library::RootModule;
use anyhow::{Context, Result, anyhow};
use kkc_plugin_api::{KKC_REMOTE_PLUGIN_API_VERSION, RemotePluginModRef};
use serde::Deserialize;
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
        let library_path = path.join(&manifest.remote.library);
        if !library_path.is_file() {
            crate::viewer::debug_log(&format!(
                "startup: native remote plugin '{}' has no built library at {}",
                manifest.plugin.id,
                library_path.display()
            ));
            continue;
        }
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
            dir: path,
        });
    }
    plugins.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(plugins)
}

#[allow(dead_code)]
pub fn load_remote_plugin(plugin_id: &str) -> Result<RemotePluginModRef> {
    let plugins_dir = crate::plugins::plugins_dir()?;
    for plugin in discover_remote_rust_plugins(&plugins_dir)? {
        if plugin.id == plugin_id {
            let manifest = read_manifest(&plugin.dir.join("plugin.toml"))?;
            return RemotePluginModRef::load_from_file(&plugin.dir.join(manifest.remote.library))
                .with_context(|| format!("Loading native remote plugin '{}'", plugin_id));
        }
    }
    Err(anyhow!(
        "Native remote plugin '{}' is not installed or built",
        plugin_id
    ))
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
