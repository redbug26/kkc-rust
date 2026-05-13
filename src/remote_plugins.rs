use abi_stable::library::lib_header_from_path;
use anyhow::{Context, Result, anyhow};
use kkc_plugin_api::{KKC_REMOTE_PLUGIN_API_VERSION, RemotePluginModRef};
use serde::Deserialize;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct RemoteRustPluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub scheme: String,
    pub config_fields: Vec<RemoteRustConfigField>,
    pub dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRustConfigField {
    pub key: String,
    pub label: String,
    pub secret: bool,
    pub required: bool,
    pub default_value: String,
}

#[derive(Debug, Clone)]
pub struct RemoteRustPluginManifestInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub dir: PathBuf,
}

#[derive(Debug, Deserialize)]
struct NativePluginManifest {
    plugin: NativePluginMetadata,
    remote: Option<NativeRemoteMetadata>,
}

#[derive(Debug, Deserialize)]
struct GenericPluginMetadata {
    #[serde(rename = "type")]
    plugin_type: String,
}

#[derive(Debug, Deserialize)]
struct GenericManifest {
    plugin: GenericPluginMetadata,
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
    discover_remote_rust_plugins_from_manifests(&manifests)
}

pub fn discover_remote_rust_plugins_from_manifests(
    manifests: &[RemoteRustPluginManifestInfo],
) -> Result<Vec<RemoteRustPluginInfo>> {
    let mut plugins = Vec::new();
    for manifest_info in manifests.iter().cloned() {
        let manifest = read_manifest(&manifest_info.dir.join("plugin.toml"))?;
        let configured_library = manifest
            .remote
            .as_ref()
            .expect("remote section")
            .library
            .clone();
        let Some(library_path) =
            resolve_remote_library_path(&manifest_info.dir, &configured_library)
        else {
            crate::viewer::debug_log(&format!(
                "startup: {} - remote-rust plugin: '{}' not found",
                manifest.plugin.id, configured_library
            ));
            continue;
        };
        crate::viewer::debug_log(&format!(
            "startup: {} - remote-rust plugin: '{}'",
            manifest.plugin.id,
            library_path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| library_path.display().to_string())
        ));
        // Use lib_header_from_path + init_root_module instead of load_from_file.
        // load_from_file caches in a process-global static (one per RootModule type),
        // so loading a second plugin of the same type would reuse the first one.
        let module = match lib_header_from_path(&library_path)
            .and_then(|h| h.init_root_module::<RemotePluginModRef>())
        {
            Ok(module) => module,
            Err(err) => {
                crate::viewer::debug_log(&format!(
                    "startup: {} - failed to load remote-rust plugin: {err}",
                    manifest.plugin.id
                ));
                continue;
            }
        };
        let api_version = module.api_version()();
        if api_version != KKC_REMOTE_PLUGIN_API_VERSION {
            crate::viewer::debug_log(&format!(
                "startup: {} - native remote plugin uses API version {}, expected {}",
                manifest.plugin.id, api_version, KKC_REMOTE_PLUGIN_API_VERSION
            ));
            continue;
        }
        let metadata = module.metadata()();
        if metadata.id.as_str() != manifest.plugin.id {
            crate::viewer::debug_log(&format!(
                "startup: {} - native remote plugin exported id '{}' (library='{}')",
                manifest.plugin.id,
                metadata.id,
                library_path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| library_path.display().to_string())
            ));
            continue;
        }
        plugins.push(RemoteRustPluginInfo {
            id: metadata.id.to_string(),
            name: metadata.name.to_string(),
            version: metadata.version.to_string(),
            description: metadata.description.to_string(),
            scheme: metadata.scheme.to_string(),
            config_fields: metadata
                .fields
                .iter()
                .map(|field| RemoteRustConfigField {
                    key: field.key.as_str().trim().to_string(),
                    label: field.label.as_str().trim().to_string(),
                    secret: field.secret,
                    required: field.required,
                    default_value: field.default_value.as_str().to_string(),
                })
                .filter(|field| !field.key.is_empty())
                .collect(),
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
        // Check plugin type first without full parsing
        let text = match fs::read_to_string(&manifest_path) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let generic: GenericManifest = match toml::from_str(&text) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if generic.plugin.plugin_type != "remote-rust" {
            continue;
        }
        // Now parse with full structure
        let manifest = match read_manifest(&manifest_path) {
            Ok(manifest) => manifest,
            Err(err) => {
                crate::viewer::debug_log(&format!(
                    "startup: remote plugin manifest parse error {} ({err})",
                    manifest_path.display()
                ));
                continue;
            }
        };
        if manifest.plugin.plugin_type != "remote-rust" {
            continue;
        }
        plugins.push(RemoteRustPluginManifestInfo {
            id: manifest.plugin.id,
            name: manifest.plugin.name,
            version: manifest.plugin.version,
            description: manifest.plugin.description,
            dir: path,
        });
    }
    plugins.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(plugins)
}

pub fn debug_log_remote_plugin_library_status(plugin_dir: &Path) -> Result<()> {
    let manifest_path = plugin_dir.join("plugin.toml");
    if !manifest_path.is_file() {
        return Ok(());
    }
    let manifest = read_manifest(&manifest_path)?;
    if manifest.plugin.plugin_type != "remote-rust" {
        return Ok(());
    }

    let configured = manifest
        .remote
        .as_ref()
        .expect("remote section")
        .library
        .clone();
    let candidates = candidate_remote_library_paths(plugin_dir, &configured);
    if let Some(found) = candidates.iter().find(|p| p.is_file()) {
        crate::viewer::debug_log(&format!(
            "remote-plugin-install: '{}' library resolved at {}",
            manifest.plugin.id,
            found.display()
        ));
    } else {
        let searched = candidates
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        crate::viewer::debug_log(&format!(
            "remote-plugin-install: '{}' installed but library '{}' was not found (searched: {})",
            manifest.plugin.id, configured, searched
        ));
    }
    Ok(())
}

#[allow(dead_code)]
pub fn load_remote_plugin(plugin_id: &str) -> Result<RemotePluginModRef> {
    let plugins_dir = crate::plugins::plugins_dir()?;
    for manifest_info in discover_remote_rust_plugin_manifests(&plugins_dir)? {
        if manifest_info.id == plugin_id {
            let manifest = read_manifest(&manifest_info.dir.join("plugin.toml"))?;
            let Some(library_path) = resolve_remote_library_path(
                &manifest_info.dir,
                &manifest.remote.as_ref().expect("remote section").library,
            ) else {
                return Err(not_found_error(
                    plugin_id,
                    &manifest_info.dir,
                    &manifest.remote.as_ref().expect("remote section").library,
                ));
            };
            return lib_header_from_path(&library_path)
                .and_then(|h| h.init_root_module::<RemotePluginModRef>())
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
        let candidate = format!("{stem}.{ext}");
        let candidate = OsString::from(candidate);
        if !out.iter().any(|existing| existing == &candidate) {
            out.push(candidate);
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
        plugin_id,
        tried
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
    if let Some(ref r) = manifest.remote {
        if r.library.trim().is_empty() {
            return Err(anyhow!(
                "{} contains an empty remote.library",
                path.display()
            ));
        }
    }
    Ok(manifest)
}
