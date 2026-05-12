use abi_stable::library::lib_header_from_path;
use anyhow::{Context, Result, anyhow};
use kkc_plugin_api::{KKC_VIEWER_PLUGIN_API_VERSION, ViewerPluginModRef};
use serde::Deserialize;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ViewerRustPluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub modes: Vec<String>,
    pub mime_types: Vec<String>,
    pub extensions: Vec<String>,
    pub dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ViewerRustPluginManifestInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub dir: PathBuf,
}

#[derive(Debug, Deserialize)]
struct NativePluginManifest {
    plugin: NativePluginMetadata,
    viewer: Option<NativeViewerMetadata>,
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
struct NativeViewerMetadata {
    library: String,
}

pub fn discover_viewer_rust_plugins(plugins_dir: &Path) -> Result<Vec<ViewerRustPluginInfo>> {
    let manifests = discover_viewer_rust_plugin_manifests(plugins_dir)?;
    let mut plugins = Vec::new();
    for manifest_info in manifests {
        let manifest = read_manifest(&manifest_info.dir.join("plugin.toml"))?;
        let Some(library_path) = resolve_viewer_library_path(
            &manifest_info.dir,
            &manifest.viewer.as_ref().expect("viewer section").library,
        ) else {
            crate::viewer::debug_log(&format!(
                "startup: native viewer plugin '{}' has no built library at {}",
                manifest.plugin.id,
                manifest_info
                    .dir
                    .join(&manifest.viewer.as_ref().expect("viewer section").library)
                    .display()
            ));
            continue;
        };

        crate::viewer::debug_log(&format!(
            "startup: loading native viewer plugin '{}' from {}",
            manifest.plugin.id,
            library_path.display()
        ));

        let module = match lib_header_from_path(&library_path)
            .and_then(|h| h.init_root_module::<ViewerPluginModRef>())
        {
            Ok(module) => module,
            Err(err) => {
                crate::viewer::debug_log(&format!(
                    "startup: failed to load native viewer plugin '{}': {err}",
                    manifest.plugin.id
                ));
                continue;
            }
        };

        let api_version = module.api_version()();
        if api_version != KKC_VIEWER_PLUGIN_API_VERSION {
            crate::viewer::debug_log(&format!(
                "Viewer plugin '{}' uses API version {}, expected {}",
                manifest.plugin.id, api_version, KKC_VIEWER_PLUGIN_API_VERSION
            ));
            continue;
        }

        let metadata = module.metadata()();
        if metadata.id.as_str() != manifest.plugin.id {
            crate::viewer::debug_log(&format!(
                "Viewer plugin '{}' exported id '{}' (loaded from {})",
                manifest.plugin.id,
                metadata.id,
                library_path.display()
            ));
            continue;
        }

        let mut modes = metadata
            .modes
            .iter()
            .map(|value| value.as_str().trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        if modes.is_empty() {
            modes.push("text".to_string());
        }
        modes.sort();
        modes.dedup();

        let mut mime_types = metadata
            .mime_types
            .iter()
            .map(|value| value.as_str().trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
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

        plugins.push(ViewerRustPluginInfo {
            id: metadata.id.to_string(),
            name: metadata.name.to_string(),
            version: metadata.version.to_string(),
            description: metadata.description.to_string(),
            modes,
            mime_types,
            extensions,
            dir: manifest_info.dir,
        });
    }
    plugins.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(plugins)
}

pub fn discover_viewer_rust_plugin_manifests(
    plugins_dir: &Path,
) -> Result<Vec<ViewerRustPluginManifestInfo>> {
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
        if manifest.plugin.plugin_type != "viewer-rust" {
            continue;
        }
        plugins.push(ViewerRustPluginManifestInfo {
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

pub fn debug_log_viewer_plugin_library_status(plugin_dir: &Path) -> Result<()> {
    let manifest_path = plugin_dir.join("plugin.toml");
    if !manifest_path.is_file() {
        return Ok(());
    }
    let manifest = read_manifest(&manifest_path)?;
    if manifest.plugin.plugin_type != "viewer-rust" {
        return Ok(());
    }

    let configured = manifest
        .viewer
        .as_ref()
        .expect("viewer section")
        .library
        .clone();
    let candidates = candidate_viewer_library_paths(plugin_dir, &configured);
    if let Some(found) = candidates.iter().find(|p| p.is_file()) {
        crate::viewer::debug_log(&format!(
            "viewer-plugin-install: '{}' library resolved at {}",
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
            "viewer-plugin-install: '{}' installed but library '{}' was not found (searched: {})",
            manifest.plugin.id, configured, searched
        ));
    }
    Ok(())
}

pub fn load_viewer_plugin(plugin_id: &str) -> Result<ViewerPluginModRef> {
    let plugins_dir = crate::plugins::plugins_dir()?;
    for manifest_info in discover_viewer_rust_plugin_manifests(&plugins_dir)? {
        if manifest_info.id == plugin_id {
            let manifest = read_manifest(&manifest_info.dir.join("plugin.toml"))?;
            let Some(library_path) = resolve_viewer_library_path(
                &manifest_info.dir,
                &manifest.viewer.as_ref().expect("viewer section").library,
            ) else {
                return Err(not_found_error(
                    plugin_id,
                    &manifest_info.dir,
                    &manifest.viewer.as_ref().expect("viewer section").library,
                ));
            };
            return lib_header_from_path(&library_path)
                .and_then(|h| h.init_root_module::<ViewerPluginModRef>())
                .with_context(|| format!("Loading native viewer plugin '{}'", plugin_id));
        }
    }
    Err(anyhow!(
        "Native viewer plugin '{}' is not installed or built",
        plugin_id
    ))
}

fn resolve_viewer_library_path(plugin_dir: &Path, configured_library: &str) -> Option<PathBuf> {
    let candidates = candidate_viewer_library_paths(plugin_dir, configured_library);
    candidates.into_iter().find(|p| p.is_file())
}

fn candidate_viewer_library_paths(plugin_dir: &Path, configured_library: &str) -> Vec<PathBuf> {
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
        let candidate = OsString::from(format!("{stem}.{ext}"));
        if !out.iter().any(|existing| existing == &candidate) {
            out.push(candidate);
        }
    }
    out
}

fn not_found_error(plugin_id: &str, plugin_dir: &Path, configured_library: &str) -> anyhow::Error {
    let tried = candidate_viewer_library_paths(plugin_dir, configured_library)
        .into_iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    anyhow!(
        "Native viewer plugin '{}' is not installed or built (searched: {})",
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
    if let Some(ref v) = manifest.viewer {
        if v.library.trim().is_empty() {
            return Err(anyhow!(
                "{} contains an empty viewer.library",
                path.display()
            ));
        }
    }
    Ok(manifest)
}
