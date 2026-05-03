use anyhow::{Context, Result, anyhow, bail};
use mlua::{Function, Lua, Table, Value};
use serde::Deserialize;
use serde::Serialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Cursor, Read, Seek};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::{OnceLock, RwLock};
use zip::ZipArchive;

const BUNDLED_LHA_LZH_PLUGIN: &str = include_str!("../assets/plugins/lha_lzh/plugin.lua");
const BUNDLED_PDF_FILE_PLUGIN: &str = include_str!("../assets/plugins/pdf_file/plugin.lua");
const BUNDLED_HTML_VIEWER_PLUGIN: &str = include_str!("../assets/plugins/html_viewer/plugin.lua");
const BUNDLED_EML_VIEWER_PLUGIN: &str = include_str!("../assets/plugins/eml_viewer/plugin.lua");
const BUNDLED_JSON_VIEWER_PLUGIN: &str = include_str!("../assets/plugins/json_viewer/plugin.lua");
const BUNDLED_XML_VIEWER_PLUGIN: &str = include_str!("../assets/plugins/xml_viewer/plugin.lua");
const BUNDLED_CSV_VIEWER_PLUGIN: &str = include_str!("../assets/plugins/csv_viewer/plugin.lua");
const BUNDLED_MARKDOWN_VIEWER_PLUGIN: &str =
    include_str!("../assets/plugins/markdown_viewer/plugin.lua");
const BUNDLED_TEXT_SYNTAX_PLUGIN: &str = include_str!("../assets/plugins/text_syntax/plugin.lua");
const BUNDLED_GIT_ACTION_PLUGIN: &str = include_str!("../assets/plugins/git_action/plugin.lua");
const BUNDLED_PLUGIN_DIRS: &[&str] = &[
    "lha_lzh",
    "pdf_file",
    "html_viewer",
    "eml_viewer",
    "json_viewer",
    "xml_viewer",
    "csv_viewer",
    "markdown_viewer",
    "text_syntax",
    "git_action",
];

static PLUGINS: OnceLock<RwLock<PluginRegistry>> = OnceLock::new();

#[derive(Debug, Deserialize)]
struct StoreIndex {
    generated_at: Option<String>,
    source_repo: Option<String>,
    plugins_count: Option<usize>,
    applications_count: Option<usize>,
    tag: Option<String>,
    #[serde(default)]
    plugins: Vec<StorePluginDescriptor>,
    #[serde(default)]
    applications: Vec<StoreApplicationDescriptor>,
}

#[derive(Debug, Deserialize)]
struct StorePluginDescriptor {
    id: String,
    name: Option<String>,
    version: Option<String>,
    #[serde(rename = "type")]
    plugin_type: Option<String>,
    description: Option<String>,
    location: StoreLocation,
}

#[derive(Debug, Deserialize)]
struct StoreLocation {
    kind: String,
    path: Option<String>,
    repo: Option<String>,
    #[serde(rename = "ref")]
    git_ref: Option<String>,
    asset_url: Option<String>,
}

#[derive(Debug, Clone)]
pub enum StoreItemKind {
    Plugin,
    Application,
}

#[derive(Debug, Deserialize)]
struct StoreApplicationDescriptor {
    id: String,
    name: Option<String>,
    version: Option<String>,
    category: Option<String>,
    #[serde(rename = "type")]
    app_type: Option<String>,
    description: Option<String>,
    #[serde(default)]
    mime_types: Vec<String>,
    #[serde(default)]
    wait_for_key_after_exit: bool,
    #[serde(default)]
    args: Option<StoreApplicationArgs>,
    #[serde(default)]
    install: Vec<StoreApplicationInstall>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum StoreApplicationArgs {
    List(Vec<String>),
    String(String),
}

#[derive(Debug, Deserialize)]
struct StoreApplicationInstall {
    os: StoreInstallOs,
    method: String,
    package: Option<String>,
    #[serde(rename = "crate")]
    crate_name: Option<String>,
    command: Option<String>,
    url: Option<String>,
    bin: Option<String>,
    #[serde(default)]
    args: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum StoreInstallOs {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Clone)]
pub struct StorePluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub plugin_type: String,
    pub description: String,
    pub item_kind: StoreItemKind,
    pub install_method: Option<String>,
    pub install_bin: Option<String>,
    pub install_methods: Vec<String>,
    pub mime_types: Vec<String>,
    pub wait_for_key_after_exit: bool,
    pub launch_args: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct StoreIndexInfo {
    pub generated_at: Option<String>,
    pub source_repo: Option<String>,
    pub plugins_count: Option<usize>,
    pub applications_count: Option<usize>,
    pub tag: Option<String>,
}

#[derive(Debug, Clone)]
struct StoreIndexSource {
    local_root: Option<PathBuf>,
    github_repo: Option<String>,
    github_ref: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PluginRegistry {
    archive_plugins: Vec<ArchivePlugin>,
    viewer_plugins: Vec<ViewerPlugin>,
    action_plugins: Vec<ActionPlugin>,
    remote_rust_plugins: Vec<crate::remote_plugins::RemoteRustPluginInfo>,
}

#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub kind: String,
    pub description: String,
    pub extensions: Vec<String>,
    pub dir: PathBuf,
}

#[derive(Debug, Clone)]
struct ArchivePlugin {
    name: String,
    version: String,
    description: String,
    script_path: PathBuf,
    plugin_dir: PathBuf,
    mime_types: Vec<String>,
    extensions: Vec<String>,
    can_add_files: bool,
}

#[derive(Debug, Clone)]
struct ViewerPlugin {
    name: String,
    version: String,
    description: String,
    script_path: PathBuf,
    plugin_dir: PathBuf,
    modes: Vec<String>,
    mime_types: Vec<String>,
    extensions: Vec<String>,
}

#[derive(Debug, Clone)]
struct ActionPlugin {
    name: String,
    version: String,
    description: String,
    script_path: PathBuf,
    plugin_dir: PathBuf,
}

#[derive(Debug, Clone)]
struct RegisteredPlugin {
    name: String,
    version: String,
    description: String,
    mime_types: Vec<String>,
    extensions: Vec<String>,
    can_add_files: bool,
}

#[derive(Debug, Clone)]
struct RegisteredViewerPlugin {
    name: String,
    version: String,
    description: String,
    modes: Vec<String>,
    mime_types: Vec<String>,
    extensions: Vec<String>,
}

#[derive(Debug, Clone)]
struct RegisteredActionPlugin {
    name: String,
    version: String,
    description: String,
}

#[derive(Debug, Clone)]
pub struct ActionItem {
    pub plugin: String,
    pub id: String,
    pub title: String,
    pub description: String,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ViewerSpan {
    pub text: String,
    pub fg: String,
    pub bg: Option<String>,
    pub bold: bool,
}

pub fn initialize() -> Result<()> {
    if PLUGINS.get().is_some() {
        crate::viewer::debug_log("startup: plugin registry already initialized");
        return Ok(());
    }

    let start = std::time::Instant::now();
    crate::viewer::debug_log("startup: plugin registry initialization begin");
    let registry = load_plugins()?;
    crate::viewer::debug_log(&format!(
        "startup: plugin registry loaded in {:.3} ms",
        start.elapsed().as_secs_f64() * 1000.0
    ));
    PLUGINS
        .set(RwLock::new(registry))
        .map_err(|_| anyhow!("Plugin registry already initialized"))?;
    Ok(())
}

pub fn supports_archive_navigation(path: &Path) -> bool {
    PLUGINS
        .get()
        .and_then(|registry| {
            registry
                .read()
                .ok()
                .map(|registry| registry.supports_archive(path))
        })
        .unwrap_or(false)
}

pub fn extract_archive_to_temp(path: &Path, destination: &Path) -> Result<bool> {
    let Some(registry) = PLUGINS.get() else {
        return Ok(false);
    };
    registry
        .read()
        .map_err(|_| anyhow!("Plugin registry lock poisoned"))?
        .extract_archive(path, destination)
}

pub fn supports_archive_add_files(path: &Path) -> bool {
    PLUGINS
        .get()
        .and_then(|registry| {
            registry
                .read()
                .ok()
                .map(|registry| registry.supports_add_files(path))
        })
        .unwrap_or(false)
}

pub fn add_files_to_archive(path: &Path, sources: &[PathBuf]) -> Result<bool> {
    let Some(registry) = PLUGINS.get() else {
        return Ok(false);
    };
    registry
        .read()
        .map_err(|_| anyhow!("Plugin registry lock poisoned"))?
        .add_files(path, sources)
}

pub fn plugins_dir() -> Result<PathBuf> {
    ensure_plugins_dir()
}

pub fn plugin_infos() -> Vec<PluginInfo> {
    let mut plugins = PLUGINS
        .get()
        .and_then(|registry| registry.read().ok().map(|registry| registry.plugin_infos()))
        .unwrap_or_default();

    if let Ok(plugins_dir) = ensure_plugins_dir() {
        let existing_remote_dirs = plugins
            .iter()
            .filter(|item| item.kind == "Remote Rust")
            .map(|item| item.dir.clone())
            .collect::<std::collections::HashSet<_>>();

        for manifest in crate::remote_plugins::discover_remote_rust_plugin_manifests(&plugins_dir)
            .unwrap_or_default()
        {
            if existing_remote_dirs.contains(&manifest.dir) {
                continue;
            }
            plugins.push(PluginInfo {
                name: manifest.name,
                version: manifest.version,
                kind: "Remote Rust".into(),
                description: format!("{} (library not loaded)", manifest.description),
                extensions: vec![manifest.id.clone()],
                dir: manifest.dir,
            });
        }
    }

    plugins
}

pub fn installed_plugin_versions_by_dir() -> HashMap<String, String> {
    let mut out = HashMap::new();
    for plugin in plugin_infos() {
        if let Some(name) = plugin
            .dir
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
        {
            out.entry(name).or_insert(plugin.version);
        }
    }

    if let Ok(plugins_root) = plugins_dir()
        && let Ok(entries) = fs::read_dir(&plugins_root)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(dir_name) = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|s| s.to_string())
            else {
                continue;
            };
            if out.contains_key(&dir_name) {
                continue;
            }
            if let Some(version) = remote_rust_manifest_version(&path) {
                out.insert(dir_name, version);
            }
        }
    }

    out
}

pub fn store_plugin_install_dir_name(plugin_id: &str) -> String {
    plugin_bundle_name(Path::new(plugin_id))
}

pub fn plugin_source_label(plugin_dir: &Path, plugins_root: &Path) -> &'static str {
    if plugin_dir_is_bundled(plugin_dir) {
        return "kkc";
    }
    if plugin_dir.starts_with(plugins_root) {
        return "store/local";
    }
    "external"
}

pub fn plugin_can_remove(plugin_dir: &Path, plugins_root: &Path) -> bool {
    if plugin_dir_is_bundled(plugin_dir) {
        return false;
    }
    plugin_dir != plugins_root
        && plugin_dir.parent() == Some(plugins_root)
        && plugin_dir.file_name().is_some()
}

pub fn remove_plugin(plugin_dir: &Path) -> Result<()> {
    let plugins_root = ensure_plugins_dir()?;
    if !plugin_can_remove(plugin_dir, &plugins_root) {
        bail!(
            "Plugin cannot be removed (bundled or external): {}",
            plugin_dir.display()
        );
    }
    if !plugin_dir.exists() {
        bail!("Plugin directory not found: {}", plugin_dir.display());
    }
    fs::remove_dir_all(plugin_dir)
        .with_context(|| format!("Removing plugin directory {}", plugin_dir.display()))?;

    let registry = load_plugins()?;
    if let Some(lock) = PLUGINS.get() {
        *lock
            .write()
            .map_err(|_| anyhow!("Plugin registry lock poisoned"))? = registry;
    } else {
        PLUGINS
            .set(RwLock::new(registry))
            .map_err(|_| anyhow!("Plugin registry already initialized"))?;
    }
    Ok(())
}

pub fn highlight_viewer_lines(
    path: &Path,
    mode: &str,
    plugin_name: &str,
    lines: &[String],
) -> Option<Vec<Vec<ViewerSpan>>> {
    PLUGINS.get().and_then(|registry| {
        registry
            .read()
            .ok()?
            .highlight_viewer_lines(path, mode, plugin_name, lines)
            .ok()
            .flatten()
    })
}

pub fn render_viewer_document(
    path: &Path,
    mode: &str,
    plugin_name: &str,
    state: &HashMap<String, String>,
    width: usize,
) -> Option<Vec<Vec<ViewerSpan>>> {
    PLUGINS.get().and_then(|registry| {
        registry
            .read()
            .ok()?
            .render_viewer_document(path, mode, plugin_name, state, width)
            .ok()
            .flatten()
    })
}

pub fn handle_viewer_key(
    path: &Path,
    mode: &str,
    plugin_name: &str,
    key: &str,
    state: &HashMap<String, String>,
) -> Option<(bool, HashMap<String, String>)> {
    PLUGINS.get().and_then(|registry| {
        registry
            .read()
            .ok()?
            .handle_viewer_key(path, mode, plugin_name, key, state)
            .ok()
            .flatten()
    })
}

pub fn viewer_plugin_infos() -> Vec<PluginInfo> {
    PLUGINS
        .get()
        .and_then(|registry| {
            registry
                .read()
                .ok()
                .map(|registry| registry.viewer_plugin_infos())
        })
        .unwrap_or_default()
}

pub fn default_viewer_plugin_for_path(path: &Path) -> Option<String> {
    PLUGINS.get().and_then(|registry| {
        registry
            .read()
            .ok()?
            .default_viewer_plugin_for_path(path)
            .map(str::to_string)
    })
}

pub fn viewer_plugins_for_path(path: &Path) -> Vec<String> {
    PLUGINS
        .get()
        .and_then(|registry| {
            let reg = registry.read().ok()?;
            let mime_type = path_mime_type(path);
            let names = reg
                .viewer_plugins
                .iter()
                .filter(|p| p.supports_path(path, mime_type.as_deref()))
                .map(|p| p.name.clone())
                .collect::<Vec<_>>();
            Some(names)
        })
        .unwrap_or_default()
}

pub fn action_items(cwd: &Path) -> Vec<ActionItem> {
    PLUGINS
        .get()
        .and_then(|registry| {
            registry
                .read()
                .ok()
                .map(|registry| registry.action_items(cwd).unwrap_or_default())
        })
        .unwrap_or_default()
}

pub fn run_action(
    plugin: &str,
    action_id: &str,
    cwd: &Path,
    input: Option<&str>,
) -> Result<String> {
    let Some(registry) = PLUGINS.get() else {
        bail!("Plugin registry is not initialized");
    };
    registry
        .read()
        .map_err(|_| anyhow!("Plugin registry lock poisoned"))?
        .run_action(plugin, action_id, cwd, input)
}

pub fn is_plugin_bundle(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("kkplug"))
        .unwrap_or(false)
}

pub fn install_plugin_bundle(path: &Path) -> Result<String> {
    let plugins_dir = ensure_plugins_dir()?;
    let installed_dir = extract_plugin_bundle(path, &plugins_dir)?;
    let registry = load_plugins()?;
    if let Some(lock) = PLUGINS.get() {
        *lock
            .write()
            .map_err(|_| anyhow!("Plugin registry lock poisoned"))? = registry;
    } else {
        PLUGINS
            .set(RwLock::new(registry))
            .map_err(|_| anyhow!("Plugin registry already initialized"))?;
    }
    Ok(installed_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("plugin")
        .to_string())
}

pub fn list_store_plugins_with_info(
    index_path: &Path,
) -> Result<(Vec<StorePluginInfo>, StoreIndexInfo)> {
    let (index, _) = read_store_index(index_path)?;
    let info = StoreIndexInfo {
        generated_at: index.generated_at.clone(),
        source_repo: index.source_repo.clone(),
        plugins_count: index.plugins_count,
        applications_count: index.applications_count,
        tag: index.tag.clone(),
    };
    let mut out = index
        .plugins
        .into_iter()
        .map(|p| StorePluginInfo {
            id: p.id,
            name: p.name.unwrap_or_else(|| "Unnamed plugin".to_string()),
            version: p.version.unwrap_or_else(|| "?".to_string()),
            plugin_type: p.plugin_type.unwrap_or_else(|| "other".to_string()),
            description: p.description.unwrap_or_default(),
            item_kind: StoreItemKind::Plugin,
            install_method: None,
            install_bin: None,
            install_methods: Vec::new(),
            mime_types: Vec::new(),
            wait_for_key_after_exit: false,
            launch_args: None,
        })
        .collect::<Vec<_>>();
    out.extend(index.applications.into_iter().map(|app| {
        let compatible_install = app
            .install
            .iter()
            .find(|method| install_os_matches(&method.os));
        StorePluginInfo {
            id: app.id,
            name: app
                .name
                .unwrap_or_else(|| "Unnamed application".to_string()),
            version: app.version.unwrap_or_else(|| "?".to_string()),
            plugin_type: app
                .app_type
                .or(app.category)
                .unwrap_or_else(|| "application".to_string()),
            description: app.description.unwrap_or_default(),
            item_kind: StoreItemKind::Application,
            install_method: compatible_install.map(store_install_method_summary),
            install_bin: compatible_install.and_then(|method| method.bin.clone()),
            install_methods: app
                .install
                .iter()
                .map(store_install_method_summary)
                .collect(),
            mime_types: app
                .mime_types
                .into_iter()
                .map(|mime| mime.to_ascii_lowercase())
                .collect(),
            wait_for_key_after_exit: app.wait_for_key_after_exit,
            launch_args: app.args.as_ref().map(store_application_args_to_command_string),
        }
    }));
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok((out, info))
}

pub fn store_index_path() -> PathBuf {
    if let Some(path) = std::env::var_os("KKC_PLUGIN_STORE_INDEX") {
        return PathBuf::from(path);
    }
    if let Some(url) = std::env::var_os("KKC_PLUGIN_STORE_URL") {
        return PathBuf::from(url);
    }
    PathBuf::from("https://raw.githubusercontent.com/redbug26/kkc-store/main/dist/store-index.json")
}

pub fn install_plugin_from_store_with_progress<F>(
    index_path: &Path,
    plugin_id: &str,
    mut progress: F,
) -> Result<String>
where
    F: FnMut(u8, &str),
{
    progress(5, "Preparing installation...");
    match install_store_item_from_store(index_path, plugin_id, &mut progress)? {
        StoreInstallResult::Plugin(installed_dir) => {
            progress(90, "Reloading plugin registry...");
            let registry = load_plugins()?;
            if let Some(lock) = PLUGINS.get() {
                *lock
                    .write()
                    .map_err(|_| anyhow!("Plugin registry lock poisoned"))? = registry;
            } else {
                PLUGINS
                    .set(RwLock::new(registry))
                    .map_err(|_| anyhow!("Plugin registry already initialized"))?;
            }
            progress(100, "Installation complete");
            Ok(installed_dir
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("plugin")
                .to_string())
        }
        StoreInstallResult::Application(name) => {
            progress(100, "Installation complete");
            Ok(name)
        }
    }
}

enum StoreInstallResult {
    Plugin(PathBuf),
    Application(String),
}

fn install_store_item_from_store(
    index_path: &Path,
    item_id: &str,
    progress: &mut dyn FnMut(u8, &str),
) -> Result<StoreInstallResult> {
    progress(12, "Reading store index...");
    let (index, source) = read_store_index(index_path)?;

    if let Some(descriptor) = index.plugins.iter().find(|p| p.id == item_id) {
        let plugins_dir = ensure_plugins_dir()?;
        return install_plugin_from_store_descriptor(
            index_path,
            &source,
            descriptor,
            &plugins_dir,
            progress,
        )
        .map(StoreInstallResult::Plugin);
    }

    if let Some(descriptor) = index.applications.iter().find(|app| app.id == item_id) {
        return install_application_from_store_descriptor(descriptor, progress)
            .map(StoreInstallResult::Application);
    }

    bail!("Store item '{}' not found in store index", item_id);
}

fn install_plugin_from_store_descriptor(
    index_path: &Path,
    source: &StoreIndexSource,
    descriptor: &StorePluginDescriptor,
    plugins_dir: &Path,
    progress: &mut dyn FnMut(u8, &str),
) -> Result<PathBuf> {
    let plugin_id = &descriptor.id;

    let install_name = plugin_bundle_name(Path::new(plugin_id));
    if plugin_name_is_bundled(&install_name) {
        bail!(
            "Store plugin '{}' conflicts with bundled plugin '{}'",
            plugin_id,
            install_name
        );
    }
    let install_dir = plugins_dir.join(&install_name);
    let temp_dir = plugins_dir.join(format!(
        ".store-install-{}-{}",
        install_name,
        std::process::id()
    ));
    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir)
            .with_context(|| format!("Cleaning {}", temp_dir.display()))?;
    }

    match descriptor.location.kind.as_str() {
        "local" => {
            progress(25, "Resolving plugin source...");
            let rel = descriptor.location.path.as_deref().ok_or_else(|| {
                anyhow!(
                    "Store local plugin '{}' is missing location.path",
                    plugin_id
                )
            })?;
            let rel = rel.trim_start_matches('/');
            if rel.is_empty() {
                bail!(
                    "Store local plugin '{}' has an empty location.path",
                    plugin_id
                );
            }
            if let Some(store_root) = source.local_root.as_ref() {
                progress(45, "Copying plugin files...");
                let src_dir = store_root.join(rel);
                copy_plugin_directory_to_temp(&src_dir, &temp_dir)?;
            } else if let Some(repo) = source.github_repo.as_deref() {
                let git_ref = source.github_ref.as_deref().unwrap_or("main");
                copy_github_plugin_path_to_temp(repo, git_ref, rel, &temp_dir, progress)?;
            } else {
                bail!(
                    "Cannot resolve local store plugin '{}' from source {}",
                    plugin_id,
                    index_path.display()
                );
            }
        }
        "github" => {
            progress(25, "Resolving plugin source...");
            if let Some(asset_url_template) = descriptor.location.asset_url.as_deref() {
                let asset_url = resolve_plugin_asset_url(asset_url_template, descriptor)?;
                progress(38, "Downloading plugin binary asset...");
                let archive_bytes = fetch_url_bytes(&asset_url)
                    .with_context(|| format!("Downloading plugin asset {}", asset_url))?;
                progress(58, "Extracting plugin binary asset...");
                extract_plugin_asset_archive(&asset_url, &archive_bytes, &temp_dir)?;
            } else {
                let repo = descriptor.location.repo.as_deref().ok_or_else(|| {
                    anyhow!(
                        "Store github plugin '{}' is missing location.repo",
                        plugin_id
                    )
                })?;
                let repo_path = descriptor.location.path.as_deref().ok_or_else(|| {
                    anyhow!(
                        "Store github plugin '{}' is missing location.path",
                        plugin_id
                    )
                })?;
                let repo_path = repo_path.trim_start_matches('/');
                if repo_path.is_empty() {
                    bail!(
                        "Store github plugin '{}' has an empty location.path",
                        plugin_id
                    );
                }
                let git_ref = descriptor
                    .location
                    .git_ref
                    .as_deref()
                    .or(source.github_ref.as_deref())
                    .unwrap_or("main");
                copy_github_plugin_path_to_temp(repo, git_ref, repo_path, &temp_dir, progress)?;
            }
        }
        other => {
            bail!(
                "Unsupported store location kind '{}' for plugin '{}': expected local or github",
                other,
                plugin_id
            )
        }
    }

    if !temp_dir.join("plugin.lua").is_file() && !is_remote_rust_plugin_dir(&temp_dir)? {
        let _ = fs::remove_dir_all(&temp_dir);
        bail!(
            "Installed store plugin '{}' does not contain plugin.lua or a valid remote-rust plugin.toml at its root",
            plugin_id
        );
    }

    if install_dir.exists() {
        progress(78, "Replacing existing plugin...");
        fs::remove_dir_all(&install_dir)
            .with_context(|| format!("Replacing {}", install_dir.display()))?;
    }
    progress(85, "Finalizing installation...");
    fs::rename(&temp_dir, &install_dir).with_context(|| {
        format!(
            "Installing plugin '{}' into {}",
            plugin_id,
            install_dir.display()
        )
    })?;

    // Log binary resolution status for remote-rust plugins installed from store.
    if let Err(err) = crate::remote_plugins::debug_log_remote_plugin_library_status(&install_dir) {
        crate::viewer::debug_log(&format!(
            "remote-plugin-install: failed to inspect '{}': {err}",
            install_dir.display()
        ));
    }
    Ok(install_dir)
}

fn install_application_from_store_descriptor(
    descriptor: &StoreApplicationDescriptor,
    progress: &mut dyn FnMut(u8, &str),
) -> Result<String> {
    progress(20, "Selecting application install method...");
    let install = descriptor
        .install
        .iter()
        .find(|method| install_os_matches(&method.os))
        .ok_or_else(|| {
            anyhow!(
                "Store application '{}' has no install method for this OS ({}). Available methods: {}",
                descriptor.id,
                current_store_os_labels().join(", "),
                store_install_methods_summary(&descriptor.install)
            )
        })?;

    let selected = store_install_method_summary(install);
    progress(28, &format!("Selected install method: {}", selected));
    progress(35, &format!("Running {}", install_command_preview(install)));
    run_application_install_command(&descriptor.id, install)?;

    if let Some(bin) = install.bin.as_deref() {
        progress(85, "Checking installed binary...");
        if !command_exists(bin) {
            bail!(
                "Store application '{}' installed with '{}', but expected binary '{}' was not found in PATH",
                descriptor.id,
                install.method,
                bin
            );
        }
    }

    Ok(descriptor
        .name
        .clone()
        .unwrap_or_else(|| descriptor.id.clone()))
}

fn run_application_install_command(app_id: &str, install: &StoreApplicationInstall) -> Result<()> {
    match install.method.as_str() {
        "cargo" => {
            let package = install
                .crate_name
                .as_deref()
                .or(install.package.as_deref())
                .ok_or_else(|| {
                    anyhow!(
                        "Store application '{}' cargo install is missing crate/package",
                        app_id
                    )
                })?;
            run_install_command(app_id, "cargo", &["install", package], &install.args)
        }
        "brew" => run_package_install_command(app_id, install, "brew", &["install"]),
        "apt" => {
            let cmd = if command_exists("apt-get") {
                "apt-get"
            } else {
                "apt"
            };
            if command_exists(cmd) {
                if !is_unix_root() && command_exists("sudo") {
                    run_package_install_command(app_id, install, "sudo", &[cmd, "install", "-y"])
                } else {
                    run_package_install_command(app_id, install, cmd, &["install", "-y"])
                }
            } else {
                bail!(
                    "Install method 'apt' for '{}' is not available on PATH",
                    app_id
                );
            }
        }
        "dnf" => {
            if !is_unix_root() && command_exists("sudo") {
                run_package_install_command(app_id, install, "sudo", &["dnf", "install", "-y"])
            } else {
                run_package_install_command(app_id, install, "dnf", &["install", "-y"])
            }
        }
        "pacman" => {
            if !is_unix_root() && command_exists("sudo") {
                run_package_install_command(
                    app_id,
                    install,
                    "sudo",
                    &["pacman", "-S", "--noconfirm"],
                )
            } else {
                run_package_install_command(app_id, install, "pacman", &["-S", "--noconfirm"])
            }
        }
        "winget" => run_package_install_command(app_id, install, "winget", &["install"]),
        "scoop" => run_package_install_command(app_id, install, "scoop", &["install"]),
        "script" => {
            let command = install.command.as_deref().ok_or_else(|| {
                anyhow!(
                    "Store application '{}' script install is missing command",
                    app_id
                )
            })?;
            run_shell_install_command(app_id, command, &install.args)
        }
        "manual" => {
            let detail = install
                .command
                .as_deref()
                .or(install.url.as_deref())
                .unwrap_or("no manual instruction provided");
            bail!(
                "Store application '{}' requires manual installation: {}",
                app_id,
                detail
            );
        }
        other => bail!(
            "Unsupported install method '{}' for store application '{}'",
            other,
            app_id
        ),
    }
}

fn store_install_methods_summary(methods: &[StoreApplicationInstall]) -> String {
    if methods.is_empty() {
        return "none".to_string();
    }
    methods
        .iter()
        .map(store_install_method_summary)
        .collect::<Vec<_>>()
        .join("; ")
}

fn store_install_method_summary(install: &StoreApplicationInstall) -> String {
    let os = install.os.values().join("/");
    let mut parts = vec![format!("{} [{}]", install.method, os)];
    if let Some(package) = install.package.as_deref() {
        parts.push(format!("package {package}"));
    }
    if let Some(crate_name) = install.crate_name.as_deref() {
        parts.push(format!("crate {crate_name}"));
    }
    if let Some(bin) = install.bin.as_deref() {
        parts.push(format!("bin {bin}"));
    }
    if let Some(url) = install.url.as_deref() {
        parts.push(url.to_string());
    }
    if let Some(command) = install.command.as_deref() {
        parts.push(command.to_string());
    }
    parts.join("  ")
}

fn install_command_preview(install: &StoreApplicationInstall) -> String {
    match install.method.as_str() {
        "cargo" => install
            .crate_name
            .as_deref()
            .or(install.package.as_deref())
            .map(|package| {
                format!(
                    "cargo install {package}{}",
                    install_args_preview(&install.args)
                )
            })
            .unwrap_or_else(|| "cargo install <missing crate>".to_string()),
        "brew" => package_command_preview("brew install", install),
        "apt" => package_command_preview("apt install -y", install),
        "dnf" => package_command_preview("dnf install -y", install),
        "pacman" => package_command_preview("pacman -S --noconfirm", install),
        "winget" => package_command_preview("winget install", install),
        "scoop" => package_command_preview("scoop install", install),
        "script" => install
            .command
            .as_deref()
            .map(|command| format!("{command}{}", install_args_preview(&install.args)))
            .unwrap_or_else(|| "<missing script command>".to_string()),
        "manual" => install
            .command
            .as_deref()
            .or(install.url.as_deref())
            .unwrap_or("<missing manual instruction>")
            .to_string(),
        other => format!("{other} <unsupported>"),
    }
}

fn package_command_preview(prefix: &str, install: &StoreApplicationInstall) -> String {
    install
        .package
        .as_deref()
        .map(|package| format!("{prefix} {package}{}", install_args_preview(&install.args)))
        .unwrap_or_else(|| format!("{prefix} <missing package>"))
}

fn install_args_preview(args: &[String]) -> String {
    if args.is_empty() {
        String::new()
    } else {
        format!(" {}", args.join(" "))
    }
}

fn store_application_args_to_command_string(args: &StoreApplicationArgs) -> String {
    match args {
        StoreApplicationArgs::String(value) => value.clone(),
        StoreApplicationArgs::List(items) => items
            .iter()
            .map(|item| shell_escape(item))
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn run_package_install_command(
    app_id: &str,
    install: &StoreApplicationInstall,
    program: &str,
    base_args: &[&str],
) -> Result<()> {
    let package = install.package.as_deref().ok_or_else(|| {
        anyhow!(
            "Store application '{}' {} install is missing package",
            app_id,
            install.method
        )
    })?;
    run_install_command(
        app_id,
        program,
        base_args,
        &[vec![package.to_string()], install.args.clone()].concat(),
    )
}

fn run_install_command(
    app_id: &str,
    program: &str,
    base_args: &[&str],
    extra_args: &[String],
) -> Result<()> {
    if !command_exists(program) {
        bail!(
            "Install command '{}' for store application '{}' is not available on PATH",
            program,
            app_id
        );
    }
    let mut command = Command::new(program);
    command.args(base_args).args(extra_args);
    let output = command
        .output()
        .with_context(|| format!("Running {} for store application '{}'", program, app_id))?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    bail!(
        "Install command '{}' for store application '{}' failed with {}: {}{}{}",
        program,
        app_id,
        output.status,
        stderr.trim(),
        if stderr.trim().is_empty() || stdout.trim().is_empty() {
            ""
        } else {
            "\n"
        },
        stdout.trim()
    );
}

fn run_shell_install_command(app_id: &str, command: &str, args: &[String]) -> Result<()> {
    let command_line = if args.is_empty() {
        command.to_string()
    } else {
        format!(
            "{} {}",
            command,
            args.iter()
                .map(|arg| shell_escape(arg))
                .collect::<Vec<_>>()
                .join(" ")
        )
    };
    if cfg!(windows) {
        run_install_command(app_id, "cmd", &["/C", &command_line], &[])
    } else {
        run_install_command(app_id, "sh", &["-c", &command_line], &[])
    }
}

fn shell_escape(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':'))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn install_os_matches(os: &StoreInstallOs) -> bool {
    let labels = current_store_os_labels();
    os.values().iter().any(|value| {
        labels
            .iter()
            .any(|label| label == &normalize_store_os(value))
    })
}

impl StoreInstallOs {
    fn values(&self) -> Vec<&str> {
        match self {
            StoreInstallOs::One(value) => vec![value.as_str()],
            StoreInstallOs::Many(values) => values.iter().map(String::as_str).collect(),
        }
    }
}

fn current_store_os_labels() -> Vec<String> {
    let mut labels = vec![normalize_store_os(std::env::consts::OS)];
    if cfg!(target_os = "linux") {
        labels.push("linux".to_string());
        labels.extend(linux_distribution_ids());
    }
    labels.sort();
    labels.dedup();
    labels
}

fn linux_distribution_ids() -> Vec<String> {
    let Ok(raw) = fs::read_to_string("/etc/os-release") else {
        return Vec::new();
    };
    raw.lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            matches!(key, "ID" | "ID_LIKE").then_some(value)
        })
        .flat_map(|value| {
            value
                .trim_matches('"')
                .split_whitespace()
                .map(normalize_store_os)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn normalize_store_os(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "darwin" | "mac" | "macosx" | "osx" => "macos".to_string(),
        "win" | "windows" => "windows".to_string(),
        other => other.to_string(),
    }
}

fn normalize_store_arch(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "x86_64" | "amd64" => "x86_64".to_string(),
        "aarch64" | "arm64" => "arm64".to_string(),
        other => other.to_string(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredApplication {
    pub id: String,
    pub name: String,
    pub version: String,
    pub bin: String,
    #[serde(default)]
    pub mime_types: Vec<String>,
    #[serde(default)]
    pub wait_for_key_after_exit: bool,
    #[serde(default)]
    pub launch_args: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct StoreConfig {
    #[serde(default)]
    applications: Vec<StoredApplication>,
}

pub fn store_config_path() -> Result<PathBuf> {
    let dirs = crate::config::project_dirs()?;
    let dir = dirs.preference_dir();
    fs::create_dir_all(dir)?;
    Ok(dir.join("store.toml"))
}

pub fn load_store_applications() -> Result<Vec<StoredApplication>> {
    let path = store_config_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path)
        .with_context(|| format!("Reading store config {}", path.display()))?;
    let cfg: StoreConfig = toml::from_str(&text)
        .with_context(|| format!("Parsing store config {}", path.display()))?;
    Ok(cfg.applications)
}

pub fn save_store_applications(applications: &[StoredApplication]) -> Result<()> {
    let path = store_config_path()?;
    let cfg = StoreConfig {
        applications: applications.to_vec(),
    };
    let text = toml::to_string_pretty(&cfg).context("Serialising store config")?;
    fs::write(&path, text).with_context(|| format!("Writing store config {}", path.display()))
}

pub fn remember_store_application(item: &StorePluginInfo) -> Result<bool> {
    if !matches!(item.item_kind, StoreItemKind::Application) {
        return Ok(false);
    }
    let Some(bin) = item.install_bin.as_deref() else {
        return Ok(false);
    };
    let mut apps = load_store_applications()?;
    let stored = StoredApplication {
        id: item.id.clone(),
        name: item.name.clone(),
        version: item.version.clone(),
        bin: bin.to_string(),
        mime_types: item.mime_types.clone(),
        wait_for_key_after_exit: item.wait_for_key_after_exit,
        launch_args: item.launch_args.clone(),
    };
    let mut changed = false;
    if let Some(existing) = apps.iter_mut().find(|app| app.id == item.id) {
        if existing.bin != stored.bin
            || existing.version != stored.version
            || existing.mime_types != stored.mime_types
            || existing.name != stored.name
            || existing.wait_for_key_after_exit != stored.wait_for_key_after_exit
            || existing.launch_args != stored.launch_args
        {
            *existing = stored;
            changed = true;
        }
    } else {
        apps.push(stored);
        changed = true;
    }
    if changed {
        save_store_applications(&apps)?;
    }
    Ok(changed)
}

pub fn remove_store_application(id: &str) -> Result<bool> {
    let mut apps = load_store_applications()?;
    let before = apps.len();
    apps.retain(|app| app.id != id);
    let changed = apps.len() != before;
    if changed {
        save_store_applications(&apps)?;
    }
    Ok(changed)
}

pub fn detect_installed_store_applications(
    items: &[StorePluginInfo],
) -> Result<Vec<StorePluginInfo>> {
    let mut remembered = load_store_applications()?;
    let mut changed = false;
    let mut detected = Vec::new();

    for item in items {
        if !matches!(item.item_kind, StoreItemKind::Application) {
            continue;
        }
        let Some(bin) = item.install_bin.as_deref() else {
            continue;
        };
        if !command_exists(bin) {
            continue;
        }
        detected.push(item.clone());
        let stored = StoredApplication {
            id: item.id.clone(),
            name: item.name.clone(),
            version: item.version.clone(),
            bin: bin.to_string(),
            mime_types: item.mime_types.clone(),
            wait_for_key_after_exit: item.wait_for_key_after_exit,
            launch_args: item.launch_args.clone(),
        };
        if let Some(existing) = remembered.iter_mut().find(|app| app.id == item.id) {
            if existing.bin != stored.bin
                || existing.version != stored.version
                || existing.mime_types != stored.mime_types
                || existing.name != stored.name
                || existing.wait_for_key_after_exit != stored.wait_for_key_after_exit
                || existing.launch_args != stored.launch_args
            {
                *existing = stored;
                changed = true;
            }
        } else {
            remembered.push(stored);
            changed = true;
        }
    }

    if changed {
        save_store_applications(&remembered)?;
    }
    Ok(detected)
}

pub fn missing_remembered_store_applications(
    items: &[StorePluginInfo],
) -> Result<Vec<StorePluginInfo>> {
    let remembered = load_store_applications()?;
    let mut missing = Vec::new();
    for stored in remembered {
        if command_exists(&stored.bin) {
            continue;
        }
        if let Some(item) = items.iter().find(|item| item.id == stored.id).cloned() {
            missing.push(item);
        } else {
            missing.push(StorePluginInfo {
                id: stored.id,
                name: stored.name,
                version: stored.version,
                plugin_type: "application".to_string(),
                description: "Remembered application missing from current store index".to_string(),
                item_kind: StoreItemKind::Application,
                install_method: None,
                install_bin: Some(stored.bin),
                install_methods: Vec::new(),
                mime_types: stored.mime_types,
                wait_for_key_after_exit: stored.wait_for_key_after_exit,
                launch_args: stored.launch_args,
            });
        }
    }
    Ok(missing)
}

pub fn store_application_launch_args_for_command(command: &str) -> Option<Option<String>> {
    let Some(program) = first_command_token(command) else {
        return None;
    };
    let program_name = Path::new(&program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program.as_str());

    load_store_applications()
        .ok()
        .and_then(|apps| {
            apps.into_iter().find_map(|app| {
                let bin_name = Path::new(&app.bin)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(app.bin.as_str());
                if app.bin == program || bin_name == program_name {
                    Some(app.launch_args)
                } else {
                    None
                }
            })
        })
}

fn first_command_token(command: &str) -> Option<String> {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    let mut out = String::new();

    for ch in command.chars() {
        if escaped {
            out.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' if in_double => escaped = true,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            c if c.is_whitespace() && !in_single && !in_double => {
                if out.is_empty() {
                    continue;
                }
                break;
            }
            _ => out.push(ch),
        }
    }

    if out.is_empty() { None } else { Some(out) }
}

pub fn store_application_waits_after_command(command: &str) -> bool {
    let Some(program) = first_command_token(command) else {
        return false;
    };
    let program_name = Path::new(&program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program.as_str());

    load_store_applications()
        .map(|apps| {
            apps.into_iter().any(|app| {
                if !app.wait_for_key_after_exit {
                    return false;
                }
                let bin_name = Path::new(&app.bin)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(app.bin.as_str());
                app.bin == program || bin_name == program_name
            })
        })
        .unwrap_or(false)
}

fn command_exists(program: &str) -> bool {
    if program.contains(std::path::MAIN_SEPARATOR) {
        return Path::new(program).is_file();
    }

    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    let extensions = if cfg!(windows) {
        std::env::var_os("PATHEXT")
            .map(|value| {
                value
                    .to_string_lossy()
                    .split(';')
                    .map(|ext| ext.to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec![".EXE".to_string(), ".BAT".to_string(), ".CMD".to_string()])
    } else {
        vec![String::new()]
    };

    std::env::split_paths(&paths).any(|dir| {
        extensions
            .iter()
            .any(|ext| dir.join(format!("{program}{ext}")).is_file())
    })
}

#[cfg(target_family = "unix")]
fn is_unix_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

#[cfg(not(target_family = "unix"))]
fn is_unix_root() -> bool {
    false
}

fn read_store_index(index_path: &Path) -> Result<(StoreIndex, StoreIndexSource)> {
    let source = index_path.to_string_lossy().to_string();
    if source.starts_with("http://") || source.starts_with("https://") {
        let raw = fetch_url_text(&source)?;
        let index: StoreIndex = serde_json::from_str(&raw)
            .with_context(|| format!("Parsing store index from {}", source))?;
        let (github_repo, github_ref) = github_repo_and_ref_from_raw_url(&source);
        Ok((
            index,
            StoreIndexSource {
                local_root: None,
                github_repo,
                github_ref,
            },
        ))
    } else {
        let raw = fs::read_to_string(index_path)
            .with_context(|| format!("Reading store index {}", index_path.display()))?;
        let index: StoreIndex = serde_json::from_str(&raw)
            .with_context(|| format!("Parsing store index {}", index_path.display()))?;
        let local_root = index_path
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        Ok((
            index,
            StoreIndexSource {
                local_root,
                github_repo: None,
                github_ref: None,
            },
        ))
    }
}

fn github_repo_and_ref_from_raw_url(url: &str) -> (Option<String>, Option<String>) {
    let https_prefix = "https://raw.githubusercontent.com/";
    if let Some(rest) = url.strip_prefix(https_prefix) {
        let mut parts = rest.split('/');
        let owner = parts.next();
        let repo = parts.next();
        let git_ref = parts.next();
        if let (Some(owner), Some(repo), Some(git_ref)) = (owner, repo, git_ref) {
            return (Some(format!("{owner}/{repo}")), Some(git_ref.to_string()));
        }
    }
    (None, None)
}

fn copy_plugin_directory_to_temp(src_dir: &Path, temp_dir: &Path) -> Result<()> {
    if !src_dir.is_dir() {
        bail!(
            "Store plugin source directory not found: {}",
            src_dir.display()
        );
    }
    fs::create_dir_all(temp_dir).with_context(|| format!("Creating {}", temp_dir.display()))?;

    for entry in walkdir::WalkDir::new(src_dir).follow_links(false) {
        let entry = entry.with_context(|| format!("Walking {}", src_dir.display()))?;
        let rel = entry
            .path()
            .strip_prefix(src_dir)
            .with_context(|| format!("Computing relative path under {}", src_dir.display()))?;
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out = temp_dir.join(rel);
        if !out.starts_with(temp_dir) {
            bail!(
                "Store plugin source contains an unsafe path: {}",
                rel.display()
            );
        }
        if entry.file_type().is_dir() {
            fs::create_dir_all(&out).with_context(|| format!("Creating {}", out.display()))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).with_context(|| format!("Creating {}", parent.display()))?;
        }
        fs::copy(entry.path(), &out)
            .with_context(|| format!("Copying {} to {}", entry.path().display(), out.display()))?;
    }

    Ok(())
}

fn copy_github_plugin_path_to_temp(
    repo: &str,
    git_ref: &str,
    repo_path: &str,
    temp_dir: &Path,
    progress: &mut dyn FnMut(u8, &str),
) -> Result<()> {
    let mut parts = repo.split('/');
    let owner = parts.next().unwrap_or_default();
    let name = parts.next().unwrap_or_default();
    if owner.is_empty() || name.is_empty() || parts.next().is_some() {
        bail!("Invalid github repo '{}': expected owner/repo", repo);
    }

    progress(35, "Downloading plugin source...");
    let zip_url = format!(
        "https://api.github.com/repos/{}/{}/zipball/{}",
        owner, name, git_ref
    );
    let zip_bytes = fetch_url_bytes(&zip_url)?;
    progress(55, "Extracting plugin archive...");
    extract_repo_path_from_zip(&zip_bytes, repo_path, temp_dir)
}

fn fetch_url_text(url: &str) -> Result<String> {
    let bytes = fetch_url_bytes(url)?;
    String::from_utf8(bytes).with_context(|| format!("Response from {} is not valid UTF-8", url))
}

fn fetch_url_bytes(url: &str) -> Result<Vec<u8>> {
    let response = match ureq::get(url).set("User-Agent", "kkc-plugin-store").call() {
        Ok(response) => response,
        Err(ureq::Error::Status(code, response)) => {
            let body = response.into_string().unwrap_or_else(|_| String::new());
            if body.trim().is_empty() {
                bail!("HTTP GET failed for {}: status {}", url, code);
            }
            bail!(
                "HTTP GET failed for {}: status {}: {}",
                url,
                code,
                body.trim()
            );
        }
        Err(err) => {
            return fetch_url_bytes_with_curl(url).with_context(|| {
                format!(
                    "HTTP GET failed for {} with ureq ({}) and curl fallback",
                    url, err
                )
            });
        }
    };

    let mut reader = response.into_reader();
    let mut buf = Vec::new();
    reader
        .read_to_end(&mut buf)
        .with_context(|| format!("Reading HTTP response from {}", url))?;
    Ok(buf)
}

fn fetch_url_bytes_with_curl(url: &str) -> Result<Vec<u8>> {
    let output = Command::new("curl")
        .args([
            "-L",
            "--fail",
            "--silent",
            "--show-error",
            "--user-agent",
            "kkc-plugin-store",
            url,
        ])
        .output()
        .context("Running curl fallback")?;

    if output.status.success() {
        Ok(output.stdout)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("curl exited with {}: {}", output.status, stderr.trim());
    }
}

fn resolve_plugin_asset_url(template: &str, descriptor: &StorePluginDescriptor) -> Result<String> {
    let version = descriptor
        .version
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| {
            anyhow!(
                "Store plugin '{}' is missing plugin.version required for asset_url",
                descriptor.id
            )
        })?;
    let os = normalize_store_os(std::env::consts::OS);
    let arch = normalize_store_arch(std::env::consts::ARCH);
    let tag = format!("v{}", version);

    Ok(template
        .replace("{version}", version)
        .replace("{tag}", &tag)
        .replace("{os}", &os)
        .replace("{arch}", &arch))
}

fn extract_plugin_asset_archive(url: &str, bytes: &[u8], temp_dir: &Path) -> Result<()> {
    if !url.to_ascii_lowercase().ends_with(".zip") {
        bail!(
            "Unsupported plugin asset format for '{}': only .zip archives are supported",
            url
        );
    }
    extract_zip_to_temp(bytes, temp_dir)
}

fn extract_zip_to_temp(zip_bytes: &[u8], temp_dir: &Path) -> Result<()> {
    fs::create_dir_all(temp_dir).with_context(|| format!("Creating {}", temp_dir.display()))?;

    let mut copied_any = false;
    let cursor = Cursor::new(zip_bytes);
    let mut archive = ZipArchive::new(cursor).context("Opening plugin asset zip archive")?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .with_context(|| format!("Reading zip entry #{}", i))?;

        let Some(enclosed_name) = entry.enclosed_name().map(PathBuf::from) else {
            bail!("Plugin asset zip contains an unsafe path: {}", entry.name());
        };
        if enclosed_name.as_os_str().is_empty() {
            continue;
        }

        let output = temp_dir.join(&enclosed_name);
        if !output.starts_with(temp_dir) {
            bail!(
                "Plugin asset zip path escapes install directory: {}",
                entry.name()
            );
        }

        if entry.is_dir() {
            fs::create_dir_all(&output)
                .with_context(|| format!("Creating {}", output.display()))?;
            continue;
        }

        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).with_context(|| format!("Creating {}", parent.display()))?;
        }
        let mut out =
            fs::File::create(&output).with_context(|| format!("Creating {}", output.display()))?;
        io::copy(&mut entry, &mut out)
            .with_context(|| format!("Extracting {} to {}", entry.name(), output.display()))?;
        copied_any = true;
    }

    if !copied_any {
        bail!("Plugin asset zip archive is empty");
    }
    Ok(())
}

fn extract_repo_path_from_zip(zip_bytes: &[u8], repo_path: &str, temp_dir: &Path) -> Result<()> {
    fs::create_dir_all(temp_dir).with_context(|| format!("Creating {}", temp_dir.display()))?;

    let normalized = repo_path.trim_matches('/');
    if normalized.is_empty() {
        bail!("Store github plugin path is empty");
    }

    let mut copied_any = false;
    let cursor = Cursor::new(zip_bytes);
    let mut archive = ZipArchive::new(cursor).context("Opening GitHub zip archive")?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .with_context(|| format!("Reading zip entry #{}", i))?;
        let entry_name = entry.name().to_string();
        if entry_name.ends_with('/') {
            continue;
        }

        let Some((_, relative_to_root)) = entry_name.split_once('/') else {
            continue;
        };
        if relative_to_root == normalized {
            continue;
        }
        let Some(relative_to_plugin) = relative_to_root.strip_prefix(normalized) else {
            continue;
        };
        let relative_to_plugin = relative_to_plugin.trim_start_matches('/');
        if relative_to_plugin.is_empty() {
            continue;
        }
        if relative_to_plugin.contains("..") {
            bail!("Unsafe zip path in plugin archive: {}", relative_to_plugin);
        }

        let out = temp_dir.join(relative_to_plugin);
        if !out.starts_with(temp_dir) {
            bail!(
                "Unsafe output path in plugin archive: {}",
                relative_to_plugin
            );
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).with_context(|| format!("Creating {}", parent.display()))?;
        }
        let mut outfile =
            fs::File::create(&out).with_context(|| format!("Creating {}", out.display()))?;
        io::copy(&mut entry, &mut outfile)
            .with_context(|| format!("Extracting {} to {}", entry_name, out.display()))?;
        copied_any = true;
    }

    if !copied_any {
        bail!(
            "Store github plugin path '{}' not found in repository zip",
            repo_path
        );
    }

    Ok(())
}

fn load_plugins() -> Result<PluginRegistry> {
    let load_start = std::time::Instant::now();
    let plugins_dir = ensure_plugins_dir()?;
    crate::viewer::debug_log(&format!("startup: plugin dir {}", plugins_dir.display()));
    let bundled_start = std::time::Instant::now();
    install_bundled_plugins(&plugins_dir)?;
    crate::viewer::debug_log(&format!(
        "startup: bundled plugins installed/updated in {:.3} ms",
        bundled_start.elapsed().as_secs_f64() * 1000.0
    ));

    let mut archive_plugins = Vec::new();
    let mut viewer_plugins = Vec::new();
    let mut action_plugins = Vec::new();
    let remote_rust_manifests = crate::remote_plugins::discover_remote_rust_plugin_manifests(
        &plugins_dir,
    )
    .unwrap_or_else(|err| {
        crate::viewer::debug_log(&format!(
            "startup: native remote plugin manifest discovery failed: {err}"
        ));
        Vec::new()
    });
    let mut remote_rust_plugins = crate::remote_plugins::discover_remote_rust_plugins(&plugins_dir)
        .unwrap_or_else(|err| {
            crate::viewer::debug_log(&format!(
                "startup: native remote plugin discovery failed: {err}"
            ));
            Vec::new()
        });
    let loaded_by_id = remote_rust_plugins
        .iter()
        .map(|plugin| plugin.id.clone())
        .collect::<std::collections::HashSet<_>>();
    for manifest in remote_rust_manifests {
        if loaded_by_id.contains(&manifest.id) {
            continue;
        }
        remote_rust_plugins.push(crate::remote_plugins::RemoteRustPluginInfo {
            id: manifest.id.clone(),
            name: manifest.name,
            version: manifest.version,
            description: format!("{} (library not loaded)", manifest.description),
            scheme: manifest.id,
            dir: manifest.dir,
        });
    }
    remote_rust_plugins.sort_by(|a, b| a.id.cmp(&b.id));
    let scripts_start = std::time::Instant::now();
    let scripts = plugin_scripts(&plugins_dir)?;
    crate::viewer::debug_log(&format!(
        "startup: discovered {} plugin script(s) in {:.3} ms",
        scripts.len(),
        scripts_start.elapsed().as_secs_f64() * 1000.0
    ));
    for script_path in scripts {
        let (registered, registered_viewers, registered_actions) = inspect_plugins(&script_path)
            .with_context(|| format!("Loading plugin {}", script_path.display()))?;
        // crate::viewer::debug_log(&format!(
        //     "startup: inspected plugin {} in {:.3} ms (archive={}, viewer={}, action={})",
        //     script_path.display(),
        //     script_start.elapsed().as_secs_f64() * 1000.0,
        //     registered.len(),
        //     registered_viewers.len(),
        //     registered_actions.len()
        // ));
        let plugin_dir = script_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| plugins_dir.clone());

        for plugin in registered {
            archive_plugins.push(ArchivePlugin {
                name: plugin.name,
                version: plugin.version,
                description: plugin.description,
                script_path: script_path.clone(),
                plugin_dir: plugin_dir.clone(),
                mime_types: plugin.mime_types,
                extensions: plugin.extensions,
                can_add_files: plugin.can_add_files,
            });
        }
        for plugin in registered_viewers {
            viewer_plugins.push(ViewerPlugin {
                name: plugin.name,
                version: plugin.version,
                description: plugin.description,
                script_path: script_path.clone(),
                plugin_dir: plugin_dir.clone(),
                modes: plugin.modes,
                mime_types: plugin.mime_types,
                extensions: plugin.extensions,
            });
        }
        for plugin in registered_actions {
            action_plugins.push(ActionPlugin {
                name: plugin.name,
                version: plugin.version,
                description: plugin.description,
                script_path: script_path.clone(),
                plugin_dir: plugin_dir.clone(),
            });
        }
    }

    crate::viewer::debug_log(&format!(
        "startup: load_plugins completed in {:.3} ms (archive={}, viewer={}, action={}, remote-rust={})",
        load_start.elapsed().as_secs_f64() * 1000.0,
        archive_plugins.len(),
        viewer_plugins.len(),
        action_plugins.len(),
        remote_rust_plugins.len()
    ));
    Ok(PluginRegistry {
        archive_plugins,
        viewer_plugins,
        action_plugins,
        remote_rust_plugins,
    })
}

fn ensure_plugins_dir() -> Result<PathBuf> {
    let plugins_dir = crate::config::data_dir()?.join("plugins");
    fs::create_dir_all(&plugins_dir)?;
    Ok(plugins_dir)
}

fn install_bundled_plugins(plugins_dir: &Path) -> Result<()> {
    let lha_dir = plugins_dir.join("lha_lzh");
    fs::create_dir_all(&lha_dir)?;
    write_bundled_file(&lha_dir.join("plugin.lua"), BUNDLED_LHA_LZH_PLUGIN)?;

    let pdf_dir = plugins_dir.join("pdf_file");
    fs::create_dir_all(&pdf_dir)?;
    write_bundled_file(&pdf_dir.join("plugin.lua"), BUNDLED_PDF_FILE_PLUGIN)?;

    let html_dir = plugins_dir.join("html_viewer");
    fs::create_dir_all(&html_dir)?;
    write_bundled_file(&html_dir.join("plugin.lua"), BUNDLED_HTML_VIEWER_PLUGIN)?;

    let eml_dir = plugins_dir.join("eml_viewer");
    fs::create_dir_all(&eml_dir)?;
    write_bundled_file(&eml_dir.join("plugin.lua"), BUNDLED_EML_VIEWER_PLUGIN)?;

    let json_dir = plugins_dir.join("json_viewer");
    fs::create_dir_all(&json_dir)?;
    write_bundled_file(&json_dir.join("plugin.lua"), BUNDLED_JSON_VIEWER_PLUGIN)?;

    let xml_dir = plugins_dir.join("xml_viewer");
    fs::create_dir_all(&xml_dir)?;
    write_bundled_file(&xml_dir.join("plugin.lua"), BUNDLED_XML_VIEWER_PLUGIN)?;

    let csv_dir = plugins_dir.join("csv_viewer");
    fs::create_dir_all(&csv_dir)?;
    write_bundled_file(&csv_dir.join("plugin.lua"), BUNDLED_CSV_VIEWER_PLUGIN)?;

    let markdown_dir = plugins_dir.join("markdown_viewer");
    fs::create_dir_all(&markdown_dir)?;
    write_bundled_file(
        &markdown_dir.join("plugin.lua"),
        BUNDLED_MARKDOWN_VIEWER_PLUGIN,
    )?;

    let syntax_dir = plugins_dir.join("text_syntax");
    fs::create_dir_all(&syntax_dir)?;
    write_bundled_file(&syntax_dir.join("plugin.lua"), BUNDLED_TEXT_SYNTAX_PLUGIN)?;

    let git_action_dir = plugins_dir.join("git_action");
    fs::create_dir_all(&git_action_dir)?;
    write_bundled_file(
        &git_action_dir.join("plugin.lua"),
        BUNDLED_GIT_ACTION_PLUGIN,
    )?;

    Ok(())
}

fn plugin_dir_is_bundled(plugin_dir: &Path) -> bool {
    plugin_dir
        .file_name()
        .and_then(|n| n.to_str())
        .map(plugin_name_is_bundled)
        .unwrap_or(false)
}

fn plugin_name_is_bundled(name: &str) -> bool {
    BUNDLED_PLUGIN_DIRS.contains(&name)
}

fn write_bundled_file(path: &Path, content: &str) -> Result<()> {
    if matches!(fs::read_to_string(path), Ok(existing) if existing == content) {
        return Ok(());
    }
    fs::write(path, content).with_context(|| format!("Writing {}", path.display()))
}

fn extract_plugin_bundle(path: &Path, plugins_dir: &Path) -> Result<PathBuf> {
    let file = fs::File::open(path).with_context(|| format!("Opening {}", path.display()))?;
    let mut archive =
        ZipArchive::new(file).with_context(|| format!("Reading {}", path.display()))?;
    let entries = plugin_bundle_entries(&mut archive)?;
    let strip_prefix = plugin_bundle_strip_prefix(&entries);
    let plugin_name = plugin_bundle_name(path);
    if plugin_name_is_bundled(&plugin_name) {
        bail!(
            "Plugin bundle '{}' conflicts with bundled plugin '{}'",
            path.display(),
            plugin_name
        );
    }
    let temp_dir = plugins_dir.join(format!(".install-{}-{}", plugin_name, std::process::id()));
    let install_dir = plugins_dir.join(&plugin_name);

    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir)
            .with_context(|| format!("Cleaning {}", temp_dir.display()))?;
    }
    fs::create_dir_all(&temp_dir).with_context(|| format!("Creating {}", temp_dir.display()))?;

    let mut has_plugin_lua = false;
    let mut has_plugin_toml = false;
    for idx in 0..archive.len() {
        let mut file = archive.by_index(idx)?;
        let Some(mut enclosed_name) = file.enclosed_name() else {
            bail!("Plugin bundle contains an unsafe path: {}", file.name());
        };
        if let Some(prefix) = &strip_prefix {
            enclosed_name = enclosed_name
                .strip_prefix(prefix)
                .with_context(|| format!("Invalid plugin bundle path {}", file.name()))?
                .to_path_buf();
        }
        if enclosed_name.as_os_str().is_empty() {
            continue;
        }
        let output = temp_dir.join(&enclosed_name);
        if !output.starts_with(&temp_dir) {
            bail!(
                "Plugin bundle path escapes install directory: {}",
                file.name()
            );
        }

        if file.is_dir() {
            fs::create_dir_all(&output)
                .with_context(|| format!("Creating {}", output.display()))?;
            continue;
        }
        if enclosed_name == Path::new("plugin.lua") {
            has_plugin_lua = true;
        }
        if enclosed_name == Path::new("plugin.toml") {
            has_plugin_toml = true;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).with_context(|| format!("Creating {}", parent.display()))?;
        }
        let mut out =
            fs::File::create(&output).with_context(|| format!("Writing {}", output.display()))?;
        io::copy(&mut file, &mut out).with_context(|| format!("Extracting {}", file.name()))?;
    }

    if !has_plugin_lua && !has_plugin_toml {
        let _ = fs::remove_dir_all(&temp_dir);
        bail!("Plugin bundle does not contain plugin.lua or plugin.toml at its root");
    }
    if !has_plugin_lua && !is_remote_rust_plugin_dir(&temp_dir)? {
        let _ = fs::remove_dir_all(&temp_dir);
        bail!("Plugin bundle contains plugin.toml but is not a valid remote-rust plugin");
    }

    if install_dir.exists() {
        fs::remove_dir_all(&install_dir)
            .with_context(|| format!("Replacing {}", install_dir.display()))?;
    }
    fs::rename(&temp_dir, &install_dir).with_context(|| {
        format!(
            "Installing {} into {}",
            path.display(),
            install_dir.display()
        )
    })?;
    Ok(install_dir)
}

fn plugin_bundle_entries<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<Vec<PathBuf>> {
    let mut entries = Vec::new();
    for idx in 0..archive.len() {
        let file = archive.by_index(idx)?;
        let Some(path) = file.enclosed_name() else {
            bail!("Plugin bundle contains an unsafe path: {}", file.name());
        };
        if !path.as_os_str().is_empty() {
            entries.push(path);
        }
    }
    Ok(entries)
}

fn plugin_bundle_strip_prefix(entries: &[PathBuf]) -> Option<PathBuf> {
    let mut first_component = None;
    for path in entries {
        let mut components = path.components();
        let first = components.next()?.as_os_str().to_os_string();
        if components.next().is_none() {
            return None;
        }
        match &first_component {
            Some(existing) if existing != &first => return None,
            None => first_component = Some(first),
            _ => {}
        }
    }
    let prefix = PathBuf::from(first_component?);
    if entries
        .iter()
        .any(|path| path == &prefix.join("plugin.lua"))
    {
        Some(prefix)
    } else {
        None
    }
}

fn plugin_bundle_name(path: &Path) -> String {
    let raw = path
        .file_stem()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("plugin");
    let mut out = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if out.is_empty() {
        out.push_str("plugin");
    }
    out
}

fn plugin_scripts(plugins_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut scripts = Vec::new();
    for entry in fs::read_dir(plugins_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let script = path.join("plugin.lua");
            if script.is_file() {
                scripts.push(script);
            }
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("lua") {
            scripts.push(path);
        }
    }
    scripts.sort();
    Ok(scripts)
}

fn is_remote_rust_plugin_dir(dir: &Path) -> Result<bool> {
    #[derive(Deserialize)]
    struct Manifest {
        plugin: ManifestPlugin,
        remote: Option<ManifestRemote>,
    }
    #[derive(Deserialize)]
    struct ManifestPlugin {
        #[serde(rename = "type")]
        plugin_type: String,
    }
    #[derive(Deserialize)]
    struct ManifestRemote {
        library: String,
    }

    let manifest_path = dir.join("plugin.toml");
    if !manifest_path.is_file() {
        return Ok(false);
    }
    let text = fs::read_to_string(&manifest_path)
        .with_context(|| format!("Reading {}", manifest_path.display()))?;
    let manifest: Manifest =
        toml::from_str(&text).with_context(|| format!("Parsing {}", manifest_path.display()))?;
    if manifest.plugin.plugin_type != "remote-rust" {
        return Ok(false);
    }
    let Some(remote) = manifest.remote else {
        bail!("remote-rust plugin is missing [remote]");
    };
    Ok(!remote.library.trim().is_empty())
}

fn remote_rust_manifest_version(dir: &Path) -> Option<String> {
    #[derive(Deserialize)]
    struct Manifest {
        plugin: ManifestPlugin,
    }
    #[derive(Deserialize)]
    struct ManifestPlugin {
        version: String,
        #[serde(rename = "type")]
        plugin_type: String,
    }

    let manifest_path = dir.join("plugin.toml");
    let text = fs::read_to_string(&manifest_path).ok()?;
    let manifest: Manifest = toml::from_str(&text).ok()?;
    if manifest.plugin.plugin_type != "remote-rust" {
        return None;
    }
    let version = manifest.plugin.version.trim();
    if version.is_empty() {
        return None;
    }
    Some(version.to_string())
}

#[cfg(test)]
fn inspect_plugin(script_path: &Path) -> Result<Vec<RegisteredPlugin>> {
    let lua = plugin_lua();
    let registered = Rc::new(RefCell::new(Vec::new()));
    install_bindings(&lua, script_path.parent().unwrap_or(Path::new("")), {
        let registered = Rc::clone(&registered);
        move |plugin| registered.borrow_mut().push(plugin)
    })?;

    let script = fs::read_to_string(script_path)?;
    lua.load(&script)
        .set_name(script_path.to_string_lossy())
        .exec()?;

    Ok(registered.borrow().clone())
}

fn inspect_plugins(
    script_path: &Path,
) -> Result<(
    Vec<RegisteredPlugin>,
    Vec<RegisteredViewerPlugin>,
    Vec<RegisteredActionPlugin>,
)> {
    let lua = plugin_lua();
    let registered_archives = Rc::new(RefCell::new(Vec::new()));
    let registered_viewers = Rc::new(RefCell::new(Vec::new()));
    let registered_actions = Rc::new(RefCell::new(Vec::new()));
    install_bindings(&lua, script_path.parent().unwrap_or(Path::new("")), {
        let registered_archives = Rc::clone(&registered_archives);
        move |plugin| registered_archives.borrow_mut().push(plugin)
    })?;
    install_viewer_registration(&lua, {
        let registered_viewers = Rc::clone(&registered_viewers);
        move |plugin| registered_viewers.borrow_mut().push(plugin)
    })?;
    install_action_registration(&lua, {
        let registered_actions = Rc::clone(&registered_actions);
        move |plugin| registered_actions.borrow_mut().push(plugin)
    })?;

    let script = fs::read_to_string(script_path)?;
    lua.load(&script)
        .set_name(script_path.to_string_lossy())
        .exec()?;

    Ok((
        registered_archives.borrow().clone(),
        registered_viewers.borrow().clone(),
        registered_actions.borrow().clone(),
    ))
}

#[cfg(test)]
fn inspect_viewer_plugin(script_path: &Path) -> Result<Vec<RegisteredViewerPlugin>> {
    let lua = plugin_lua();
    let registered = Rc::new(RefCell::new(Vec::new()));
    install_bindings(&lua, script_path.parent().unwrap_or(Path::new("")), |_| {})?;
    install_viewer_registration(&lua, {
        let registered = Rc::clone(&registered);
        move |plugin| registered.borrow_mut().push(plugin)
    })?;

    let script = fs::read_to_string(script_path)?;
    lua.load(&script)
        .set_name(script_path.to_string_lossy())
        .exec()?;

    Ok(registered.borrow().clone())
}

#[cfg(test)]
fn inspect_action_plugin(script_path: &Path) -> Result<Vec<RegisteredActionPlugin>> {
    let lua = plugin_lua();
    let registered = Rc::new(RefCell::new(Vec::new()));
    install_bindings(&lua, script_path.parent().unwrap_or(Path::new("")), |_| {})?;
    install_action_registration(&lua, {
        let registered = Rc::clone(&registered);
        move |plugin| registered.borrow_mut().push(plugin)
    })?;

    let script = fs::read_to_string(script_path)?;
    lua.load(&script)
        .set_name(script_path.to_string_lossy())
        .exec()?;

    Ok(registered.borrow().clone())
}

fn install_bindings<F>(lua: &Lua, plugin_dir: &Path, on_register: F) -> Result<()>
where
    F: Fn(RegisteredPlugin) + 'static,
{
    let globals = lua.globals();
    let package: Table = globals.get("package")?;
    let current_path: String = package.get("path")?;
    package.set(
        "path",
        format!(
            "{}/?.lua;{}/?/init.lua;{}",
            plugin_dir.display(),
            plugin_dir.display(),
            current_path
        ),
    )?;

    let sj = lua.create_table()?;
    sj.set(
        "error",
        lua.create_function(|_, message: String| Err::<(), _>(mlua::Error::external(message)))?,
    )?;
    globals.set("sj", sj)?;

    let kkc = lua.create_table()?;
    kkc.set(
        "register_archive_plugin",
        lua.create_function(move |_, table: Table| {
            let name: String = table.get("name")?;
            let version: String = table.get("version").unwrap_or_else(|_| "0.0.0".into());
            let description: String = table.get("description").unwrap_or_else(|_| String::new());
            let extract: Option<Function> = table.get("extract")?;
            let add_files: Option<Function> = table.get("add_files").ok();
            if extract.is_none() {
                return Err(mlua::Error::external(format!(
                    "Plugin '{name}' does not define extract()"
                )));
            }

            let mime_types = table_string_list(&table, "mime_types")?
                .into_iter()
                .map(|value| value.to_ascii_lowercase())
                .collect::<Vec<_>>();
            let extensions = table_string_list(&table, "extensions")?
                .into_iter()
                .map(|ext| ext.trim_start_matches('.').to_ascii_lowercase())
                .collect::<Vec<_>>();

            on_register(RegisteredPlugin {
                name,
                version,
                description,
                mime_types,
                extensions,
                can_add_files: add_files.is_some(),
            });
            Ok(())
        })?,
    )?;
    kkc.set(
        "register_viewer_plugin",
        lua.create_function(|_, _: Table| Ok(()))?,
    )?;
    kkc.set(
        "register_action_plugin",
        lua.create_function(|_, _: Table| Ok(()))?,
    )?;
    kkc.set(
        "path_join",
        lua.create_function(|_, (base, child): (String, String)| {
            Ok(Path::new(&base).join(child).to_string_lossy().into_owned())
        })?,
    )?;
    kkc.set(
        "create_dir_all",
        lua.create_function(|_, path: String| {
            fs::create_dir_all(path).map_err(mlua::Error::external)
        })?,
    )?;
    kkc.set(
        "is_dir",
        lua.create_function(|_, path: String| Ok(Path::new(&path).is_dir()))?,
    )?;
    kkc.set(
        "path_exists",
        lua.create_function(|_, path: String| Ok(Path::new(&path).exists()))?,
    )?;
    kkc.set(
        "debug_log",
        lua.create_function(|_, message: String| {
            crate::viewer::debug_log(&format!("plugin: {message}"));
            Ok(())
        })?,
    )?;
    kkc.set(
        "exec",
        lua.create_function(
            |lua, (program, args, cwd): (String, Option<Table>, Option<String>)| {
                let mut command = std::process::Command::new(program);
                if let Some(args) = args {
                    for arg in args.sequence_values::<String>() {
                        command.arg(arg?);
                    }
                }
                if let Some(cwd) = cwd {
                    command.current_dir(cwd);
                }
                let output = command.output().map_err(mlua::Error::external)?;
                let result = lua.create_table()?;
                result.set("status", output.status.code().unwrap_or(-1))?;
                result.set("success", output.status.success())?;
                result.set(
                    "stdout",
                    String::from_utf8_lossy(&output.stdout).to_string(),
                )?;
                result.set(
                    "stderr",
                    String::from_utf8_lossy(&output.stderr).to_string(),
                )?;
                Ok(result)
            },
        )?,
    )?;
    kkc.set(
        "write_file",
        lua.create_function(|_, (path, content): (String, mlua::String)| {
            if let Some(parent) = Path::new(&path).parent() {
                fs::create_dir_all(parent).map_err(mlua::Error::external)?;
            }
            fs::write(path, content.as_bytes()).map_err(mlua::Error::external)
        })?,
    )?;
    let preload: Table = package.get("preload")?;
    preload.set("kkc", lua.create_function(move |_, ()| Ok(kkc.clone()))?)?;

    Ok(())
}

fn table_string_list(table: &Table, key: &str) -> mlua::Result<Vec<String>> {
    match table.get::<Option<Table>>(key)? {
        Some(values) => values.sequence_values::<String>().collect(),
        None => Ok(Vec::new()),
    }
}

impl PluginRegistry {
    fn plugin_infos(&self) -> Vec<PluginInfo> {
        let mut plugins = self
            .archive_plugins
            .iter()
            .map(|plugin| PluginInfo {
                name: plugin.name.clone(),
                version: plugin.version.clone(),
                kind: "Archive".into(),
                description: plugin.description.clone(),
                extensions: plugin.support_labels(),
                dir: plugin.plugin_dir.clone(),
            })
            .collect::<Vec<_>>();
        plugins.extend(self.viewer_plugins.iter().map(|plugin| PluginInfo {
            name: plugin.name.clone(),
            version: plugin.version.clone(),
            kind: "Viewer".into(),
            description: plugin.description.clone(),
            extensions: if !plugin.mime_types.is_empty() {
                plugin.mime_types.clone()
            } else if plugin.extensions.is_empty() {
                plugin.modes.clone()
            } else {
                plugin.extensions.clone()
            },
            dir: plugin.plugin_dir.clone(),
        }));
        plugins.extend(self.action_plugins.iter().map(|plugin| PluginInfo {
            name: plugin.name.clone(),
            version: plugin.version.clone(),
            kind: "Action".into(),
            description: plugin.description.clone(),
            extensions: Vec::new(),
            dir: plugin.plugin_dir.clone(),
        }));
        plugins.extend(self.remote_rust_plugins.iter().map(|plugin| PluginInfo {
            name: plugin.name.clone(),
            version: plugin.version.clone(),
            kind: "Remote Rust".into(),
            description: plugin.description.clone(),
            extensions: vec![plugin.scheme.clone()],
            dir: plugin.dir.clone(),
        }));
        plugins
    }

    fn viewer_plugin_infos(&self) -> Vec<PluginInfo> {
        self.viewer_plugins
            .iter()
            .map(|plugin| PluginInfo {
                name: plugin.name.clone(),
                version: plugin.version.clone(),
                kind: "Viewer".into(),
                description: plugin.description.clone(),
                extensions: if !plugin.mime_types.is_empty() {
                    plugin.mime_types.clone()
                } else if plugin.extensions.is_empty() {
                    plugin.modes.clone()
                } else {
                    plugin.extensions.clone()
                },
                dir: plugin.plugin_dir.clone(),
            })
            .collect()
    }

    fn default_viewer_plugin_for_path(&self, path: &Path) -> Option<&str> {
        let mime_type = path_mime_type(path);
        self.viewer_plugins
            .iter()
            .find(|plugin| plugin.supports_path(path, mime_type.as_deref()))
            .map(|plugin| plugin.name.as_str())
    }

    fn supports_archive(&self, path: &Path) -> bool {
        let mime_type = path_mime_type(path);
        self.archive_plugins
            .iter()
            .any(|plugin| plugin.supports_path(path, mime_type.as_deref()))
    }

    fn supports_add_files(&self, path: &Path) -> bool {
        let mime_type = path_mime_type(path);
        self.archive_plugins
            .iter()
            .any(|plugin| plugin.supports_path(path, mime_type.as_deref()) && plugin.can_add_files)
    }

    fn extract_archive(&self, path: &Path, destination: &Path) -> Result<bool> {
        let mime_type = path_mime_type(path);
        let Some(plugin) = self
            .archive_plugins
            .iter()
            .find(|plugin| plugin.supports_path(path, mime_type.as_deref()))
        else {
            return Ok(false);
        };

        plugin.extract(path, destination)?;
        Ok(true)
    }

    fn add_files(&self, path: &Path, sources: &[PathBuf]) -> Result<bool> {
        let mime_type = path_mime_type(path);
        let Some(plugin) = self.archive_plugins.iter().find(|plugin| {
            plugin.supports_path(path, mime_type.as_deref()) && plugin.can_add_files
        }) else {
            return Ok(false);
        };

        plugin.add_files(path, sources)?;
        Ok(true)
    }

    fn highlight_viewer_lines(
        &self,
        path: &Path,
        mode: &str,
        plugin_name: &str,
        lines: &[String],
    ) -> Result<Option<Vec<Vec<ViewerSpan>>>> {
        let Some(plugin) = self
            .viewer_plugins
            .iter()
            .find(|plugin| plugin.name == plugin_name && plugin.supports_mode(mode))
        else {
            return Ok(None);
        };
        plugin.highlight_lines(path, mode, lines)
    }

    fn render_viewer_document(
        &self,
        path: &Path,
        mode: &str,
        plugin_name: &str,
        state: &HashMap<String, String>,
        width: usize,
    ) -> Result<Option<Vec<Vec<ViewerSpan>>>> {
        let Some(plugin) = self
            .viewer_plugins
            .iter()
            .find(|plugin| plugin.name == plugin_name && plugin.supports_mode(mode))
        else {
            return Ok(None);
        };
        plugin.render_document(path, mode, state, width)
    }

    fn handle_viewer_key(
        &self,
        path: &Path,
        mode: &str,
        plugin_name: &str,
        key: &str,
        state: &HashMap<String, String>,
    ) -> Result<Option<(bool, HashMap<String, String>)>> {
        let Some(plugin) = self
            .viewer_plugins
            .iter()
            .find(|plugin| plugin.name == plugin_name && plugin.supports_mode(mode))
        else {
            return Ok(None);
        };
        plugin.handle_key(path, mode, key, state)
    }

    fn action_items(&self, cwd: &Path) -> Result<Vec<ActionItem>> {
        let mut actions = Vec::new();
        for plugin in &self.action_plugins {
            actions.extend(plugin.discover(cwd)?);
        }
        Ok(actions)
    }

    fn run_action(
        &self,
        plugin_name: &str,
        action_id: &str,
        cwd: &Path,
        input: Option<&str>,
    ) -> Result<String> {
        let Some(plugin) = self
            .action_plugins
            .iter()
            .find(|plugin| plugin.name == plugin_name)
        else {
            bail!("Action plugin '{}' is not registered", plugin_name);
        };
        plugin.run(action_id, cwd, input)
    }
}

impl ArchivePlugin {
    fn supports_path(&self, path: &Path, mime_type: Option<&str>) -> bool {
        supports_path_mime_or_legacy_ext(path, mime_type, &self.mime_types, &self.extensions)
    }

    fn support_labels(&self) -> Vec<String> {
        if self.mime_types.is_empty() {
            self.extensions.clone()
        } else {
            self.mime_types.clone()
        }
    }

    fn extract(&self, path: &Path, destination: &Path) -> Result<()> {
        let lua = plugin_lua();
        let handles = Rc::new(RefCell::new(Vec::new()));
        install_runtime_bindings(&lua, &self.plugin_dir, Rc::clone(&handles))?;

        let script = fs::read_to_string(&self.script_path)?;
        lua.load(&script)
            .set_name(self.script_path.to_string_lossy())
            .exec()?;

        let handles = handles.borrow();
        for key in handles.iter() {
            let table: Table = lua.registry_value(key)?;
            let name: String = table.get("name")?;
            if name == self.name {
                let extract: Function = table.get("extract")?;
                let ok: bool = extract.call((
                    path.to_string_lossy().into_owned(),
                    destination.to_string_lossy().into_owned(),
                ))?;
                if !ok {
                    bail!(
                        "Plugin '{}' failed to extract {}",
                        self.name,
                        path.display()
                    );
                }
                return Ok(());
            }
        }

        bail!("Plugin '{}' was not registered at runtime", self.name)
    }

    fn add_files(&self, path: &Path, sources: &[PathBuf]) -> Result<()> {
        let lua = plugin_lua();
        let handles = Rc::new(RefCell::new(Vec::new()));
        install_runtime_bindings(&lua, &self.plugin_dir, Rc::clone(&handles))?;

        let script = fs::read_to_string(&self.script_path)?;
        lua.load(&script)
            .set_name(self.script_path.to_string_lossy())
            .exec()?;

        let source_table = lua.create_table()?;
        for (idx, source) in sources.iter().enumerate() {
            source_table.set(idx + 1, source.to_string_lossy().into_owned())?;
        }

        let handles = handles.borrow();
        for key in handles.iter() {
            let table: Table = lua.registry_value(key)?;
            let name: String = table.get("name")?;
            if name == self.name {
                let add_files: Function = table.get("add_files")?;
                let ok: bool =
                    add_files.call((path.to_string_lossy().into_owned(), source_table))?;
                if !ok {
                    bail!(
                        "Plugin '{}' failed to add files to {}",
                        self.name,
                        path.display()
                    );
                }
                return Ok(());
            }
        }

        bail!("Plugin '{}' was not registered at runtime", self.name)
    }
}

impl ViewerPlugin {
    fn supports_mode(&self, mode: &str) -> bool {
        self.modes.iter().any(|candidate| candidate == mode)
    }

    fn supports_path(&self, path: &Path, mime_type: Option<&str>) -> bool {
        supports_path_mime_or_legacy_ext(path, mime_type, &self.mime_types, &self.extensions)
    }

    fn highlight_lines(
        &self,
        path: &Path,
        mode: &str,
        lines: &[String],
    ) -> Result<Option<Vec<Vec<ViewerSpan>>>> {
        let lua = plugin_lua();
        let handles = Rc::new(RefCell::new(Vec::new()));
        install_bindings(&lua, &self.plugin_dir, |_| {})?;
        install_runtime_viewer_bindings(&lua, Rc::clone(&handles))?;

        let script = fs::read_to_string(&self.script_path)?;
        lua.load(&script)
            .set_name(self.script_path.to_string_lossy())
            .exec()?;

        let handles = handles.borrow();
        for key in handles.iter() {
            let table: Table = lua.registry_value(key)?;
            let name: String = table.get("name")?;
            if name == self.name {
                let Some(render_line) = table.get::<Option<Function>>("render_line")? else {
                    return Ok(None);
                };
                let path = path.to_string_lossy().into_owned();
                let mut highlighted = Vec::with_capacity(lines.len());
                for line in lines {
                    let result: Option<Table> =
                        render_line.call((path.clone(), mode.to_string(), line.clone()))?;
                    let Some(spans) = result else {
                        return Ok(None);
                    };
                    highlighted.push(lua_spans_to_viewer_spans(spans)?);
                }
                return Ok(Some(highlighted));
            }
        }

        Ok(None)
    }

    fn render_document(
        &self,
        path: &Path,
        mode: &str,
        state: &HashMap<String, String>,
        width: usize,
    ) -> Result<Option<Vec<Vec<ViewerSpan>>>> {
        let lua = plugin_lua();
        let handles = Rc::new(RefCell::new(Vec::new()));
        install_bindings(&lua, &self.plugin_dir, |_| {})?;
        install_runtime_viewer_bindings(&lua, Rc::clone(&handles))?;

        let script = fs::read_to_string(&self.script_path)?;
        lua.load(&script)
            .set_name(self.script_path.to_string_lossy())
            .exec()?;

        let state_table = lua_state_table(&lua, state)?;
        let handles = handles.borrow();
        for key in handles.iter() {
            let table: Table = lua.registry_value(key)?;
            let name: String = table.get("name")?;
            if name == self.name {
                let Some(render) = table.get::<Option<Function>>("render")? else {
                    return Ok(None);
                };

                crate::viewer::debug_log(&format!(
                    "Rendering document with plugin '{}'",
                    self.name
                ));

                let result: Option<Table> = render.call((
                    path.to_string_lossy().into_owned(),
                    mode.to_string(),
                    state_table.clone(),
                    width as u64,
                ))?;
                let Some(lines) = result else {
                    return Ok(None);
                };
                return lua_lines_to_viewer_spans(lines).map(Some);
            }
        }

        Ok(None)
    }

    fn handle_key(
        &self,
        path: &Path,
        mode: &str,
        key: &str,
        state: &HashMap<String, String>,
    ) -> Result<Option<(bool, HashMap<String, String>)>> {
        let lua = plugin_lua();
        let handles = Rc::new(RefCell::new(Vec::new()));
        install_bindings(&lua, &self.plugin_dir, |_| {})?;
        install_runtime_viewer_bindings(&lua, Rc::clone(&handles))?;

        let script = fs::read_to_string(&self.script_path)?;
        lua.load(&script)
            .set_name(self.script_path.to_string_lossy())
            .exec()?;

        let state_table = lua_state_table(&lua, state)?;
        let handles = handles.borrow();
        for reg_key in handles.iter() {
            let table: Table = lua.registry_value(reg_key)?;
            let name: String = table.get("name")?;
            if name == self.name {
                let Some(handle_key_fn) = table.get::<Option<Function>>("handle_key")? else {
                    return Ok(None);
                };
                let result: Option<Table> = handle_key_fn.call((
                    path.to_string_lossy().into_owned(),
                    mode.to_string(),
                    key.to_string(),
                    state_table,
                ))?;
                let Some(result_table) = result else {
                    return Ok(None);
                };
                let consumed: bool = result_table.get("consumed").unwrap_or(false);
                let new_state_table: Table = result_table.get("state")?;
                return Ok(Some((consumed, lua_table_to_state(new_state_table)?)));
            }
        }

        Ok(None)
    }
}

impl ActionPlugin {
    fn discover(&self, cwd: &Path) -> Result<Vec<ActionItem>> {
        let lua = plugin_lua();
        let handles = Rc::new(RefCell::new(Vec::new()));
        install_bindings(&lua, &self.plugin_dir, |_| {})?;
        install_runtime_action_bindings(&lua, Rc::clone(&handles))?;

        let script = fs::read_to_string(&self.script_path)?;
        lua.load(&script)
            .set_name(self.script_path.to_string_lossy())
            .exec()?;

        let handles = handles.borrow();
        for key in handles.iter() {
            let table: Table = lua.registry_value(key)?;
            let name: String = table.get("name")?;
            if name == self.name {
                let discover: Function = table.get("discover")?;
                let result: Option<Table> = discover.call(cwd.to_string_lossy().into_owned())?;
                let Some(result) = result else {
                    return Ok(Vec::new());
                };
                return lua_action_items(result, &self.name);
            }
        }

        Ok(Vec::new())
    }

    fn run(&self, action_id: &str, cwd: &Path, input: Option<&str>) -> Result<String> {
        let lua = plugin_lua();
        let handles = Rc::new(RefCell::new(Vec::new()));
        install_bindings(&lua, &self.plugin_dir, |_| {})?;
        install_runtime_action_bindings(&lua, Rc::clone(&handles))?;

        let script = fs::read_to_string(&self.script_path)?;
        lua.load(&script)
            .set_name(self.script_path.to_string_lossy())
            .exec()?;

        let handles = handles.borrow();
        for key in handles.iter() {
            let table: Table = lua.registry_value(key)?;
            let name: String = table.get("name")?;
            if name == self.name {
                let run: Function = table.get("run")?;
                let result: Value = run.call((
                    cwd.to_string_lossy().into_owned(),
                    action_id.to_string(),
                    input.unwrap_or("").to_string(),
                ))?;
                return lua_action_result(result);
            }
        }

        bail!(
            "Action plugin '{}' was not registered at runtime",
            self.name
        )
    }
}

fn lua_action_items(table: Table, plugin_name: &str) -> Result<Vec<ActionItem>> {
    table
        .sequence_values::<Table>()
        .map(|item| {
            let item = item?;
            Ok(ActionItem {
                plugin: plugin_name.to_string(),
                id: item.get("id")?,
                title: item.get("title")?,
                description: item.get("description").unwrap_or_else(|_| String::new()),
                prompt: item.get("prompt").ok(),
            })
        })
        .collect::<mlua::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn lua_action_result(value: Value) -> Result<String> {
    match value {
        Value::String(text) => Ok(text.to_str()?.to_string()),
        Value::Table(table) => {
            let ok = table.get("ok").unwrap_or(true);
            let message = table.get("message").unwrap_or_else(|_| String::new());
            if ok { Ok(message) } else { bail!(message) }
        }
        Value::Nil => Ok(String::new()),
        _ => bail!("Action plugin returned an unsupported result"),
    }
}

fn lua_state_table(lua: &Lua, state: &HashMap<String, String>) -> Result<Table> {
    let t = lua.create_table()?;
    for (k, v) in state {
        t.set(k.clone(), v.clone())?;
    }
    Ok(t)
}

fn lua_table_to_state(table: Table) -> Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    for pair in table.pairs::<Value, Value>() {
        if let Ok((k, v)) = pair {
            let k_str = match k {
                Value::String(s) => Some(s.to_str()?.to_string()),
                Value::Integer(i) => Some(i.to_string()),
                _ => None,
            };
            let v_str = match v {
                Value::String(s) => Some(s.to_str()?.to_string()),
                Value::Integer(i) => Some(i.to_string()),
                Value::Number(n) => Some(n.to_string()),
                Value::Boolean(b) => Some(b.to_string()),
                _ => None,
            };
            if let (Some(k), Some(v)) = (k_str, v_str) {
                map.insert(k, v);
            }
        }
    }
    Ok(map)
}

fn lua_lines_to_viewer_spans(table: Table) -> Result<Vec<Vec<ViewerSpan>>> {
    table
        .sequence_values::<Value>()
        .map(|line| match line? {
            Value::String(text) => Ok(vec![ViewerSpan {
                text: text.to_str()?.to_string(),
                fg: "white".into(),
                bg: Some("black".into()),
                bold: false,
            }]),
            Value::Table(spans) if spans.contains_key("text")? => {
                Ok(vec![lua_span_to_viewer_span(spans)?])
            }
            Value::Table(spans) => lua_spans_to_viewer_spans(spans),
            _ => Ok(Vec::new()),
        })
        .collect()
}

fn lua_spans_to_viewer_spans(table: Table) -> Result<Vec<ViewerSpan>> {
    table
        .sequence_values::<Table>()
        .map(|span| lua_span_to_viewer_span(span?))
        .collect::<mlua::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn lua_span_to_viewer_span(span: Table) -> mlua::Result<ViewerSpan> {
    Ok(ViewerSpan {
        text: span.get("text")?,
        fg: span.get("fg").unwrap_or_else(|_| "white".into()),
        bg: span.get("bg").ok(),
        bold: span.get("bold").unwrap_or(false),
    })
}

fn path_mime_type(path: &Path) -> Option<String> {
    crate::idf::probe_path(path).map(|info| info.mime_type.to_ascii_lowercase())
}

fn supports_path_mime_or_legacy_ext(
    path: &Path,
    mime_type: Option<&str>,
    mime_types: &[String],
    extensions: &[String],
) -> bool {
    if let Some(mime_type) = mime_type
        && mime_types.iter().any(|candidate| candidate == mime_type)
    {
        return true;
    }

    if !mime_types.is_empty() {
        return false;
    }

    let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    let ext = ext.to_ascii_lowercase();
    extensions.iter().any(|candidate| candidate == &ext)
}

fn plugin_lua() -> Lua {
    // Archive plugins need Lua's standard IO library; dsk-lua uses io.open().
    unsafe { Lua::unsafe_new() }
}

fn install_runtime_bindings(
    lua: &Lua,
    plugin_dir: &Path,
    handles: Rc<RefCell<Vec<mlua::RegistryKey>>>,
) -> Result<()> {
    install_bindings(lua, plugin_dir, move |plugin| {
        let _ = plugin;
    })?;

    let globals = lua.globals();
    let package: Table = globals.get("package")?;
    let preload: Table = package.get("preload")?;
    let kkc_loader: Function = preload.get("kkc")?;
    let kkc: Table = kkc_loader.call(())?;
    kkc.set(
        "register_archive_plugin",
        lua.create_function(move |lua, table: Table| {
            let extract: Option<Function> = table.get("extract")?;
            if extract.is_none() {
                return Err(mlua::Error::external("Plugin does not define extract()"));
            }
            handles.borrow_mut().push(lua.create_registry_value(table)?);
            Ok(())
        })?,
    )?;
    preload.set("kkc", lua.create_function(move |_, ()| Ok(kkc.clone()))?)?;

    Ok(())
}

fn install_viewer_registration<F>(lua: &Lua, on_register: F) -> Result<()>
where
    F: Fn(RegisteredViewerPlugin) + 'static,
{
    let globals = lua.globals();
    let package: Table = globals.get("package")?;
    let preload: Table = package.get("preload")?;
    let kkc_loader: Function = preload.get("kkc")?;
    let kkc: Table = kkc_loader.call(())?;
    kkc.set(
        "register_viewer_plugin",
        lua.create_function(move |_, table: Table| {
            let name: String = table.get("name")?;
            let version: String = table.get("version").unwrap_or_else(|_| "0.0.0".into());
            let description: String = table.get("description").unwrap_or_else(|_| String::new());
            let render_line: Option<Function> = table.get("render_line").ok();
            let render: Option<Function> = table.get("render").ok();
            if render_line.is_none() && render.is_none() {
                return Err(mlua::Error::external(format!(
                    "Viewer plugin '{name}' does not define render_line() or render()"
                )));
            }
            let modes = match table.get::<Option<Table>>("modes")? {
                Some(values) => values
                    .sequence_values::<String>()
                    .collect::<mlua::Result<Vec<_>>>()?,
                None => Vec::new(),
            };
            let mime_types = table_string_list(&table, "mime_types")?
                .into_iter()
                .map(|value| value.to_ascii_lowercase())
                .collect::<Vec<_>>();
            let extensions = table_string_list(&table, "extensions")?
                .into_iter()
                .map(|ext| ext.trim_start_matches('.').to_ascii_lowercase())
                .collect::<Vec<_>>();
            on_register(RegisteredViewerPlugin {
                name,
                version,
                description,
                modes,
                mime_types,
                extensions,
            });
            Ok(())
        })?,
    )?;
    preload.set("kkc", lua.create_function(move |_, ()| Ok(kkc.clone()))?)?;
    Ok(())
}

fn install_runtime_viewer_bindings(
    lua: &Lua,
    handles: Rc<RefCell<Vec<mlua::RegistryKey>>>,
) -> Result<()> {
    let globals = lua.globals();
    let package: Table = globals.get("package")?;
    let preload: Table = package.get("preload")?;
    let kkc_loader: Function = preload.get("kkc")?;
    let kkc: Table = kkc_loader.call(())?;
    kkc.set(
        "register_viewer_plugin",
        lua.create_function(move |lua, table: Table| {
            let render_line: Option<Function> = table.get("render_line").ok();
            let render: Option<Function> = table.get("render").ok();
            if render_line.is_none() && render.is_none() {
                return Err(mlua::Error::external(
                    "Viewer plugin does not define render_line() or render()",
                ));
            }
            handles.borrow_mut().push(lua.create_registry_value(table)?);
            Ok(())
        })?,
    )?;
    preload.set("kkc", lua.create_function(move |_, ()| Ok(kkc.clone()))?)?;
    Ok(())
}

fn install_action_registration<F>(lua: &Lua, on_register: F) -> Result<()>
where
    F: Fn(RegisteredActionPlugin) + 'static,
{
    let globals = lua.globals();
    let package: Table = globals.get("package")?;
    let preload: Table = package.get("preload")?;
    let kkc_loader: Function = preload.get("kkc")?;
    let kkc: Table = kkc_loader.call(())?;
    kkc.set(
        "register_action_plugin",
        lua.create_function(move |_, table: Table| {
            let name: String = table.get("name")?;
            let version: String = table.get("version").unwrap_or_else(|_| "0.0.0".into());
            let description: String = table.get("description").unwrap_or_else(|_| String::new());
            let discover: Option<Function> = table.get("discover").ok();
            let run: Option<Function> = table.get("run").ok();
            if discover.is_none() || run.is_none() {
                return Err(mlua::Error::external(format!(
                    "Action plugin '{name}' does not define discover() and run()"
                )));
            }
            on_register(RegisteredActionPlugin {
                name,
                version,
                description,
            });
            Ok(())
        })?,
    )?;
    preload.set("kkc", lua.create_function(move |_, ()| Ok(kkc.clone()))?)?;
    Ok(())
}

fn install_runtime_action_bindings(
    lua: &Lua,
    handles: Rc<RefCell<Vec<mlua::RegistryKey>>>,
) -> Result<()> {
    let globals = lua.globals();
    let package: Table = globals.get("package")?;
    let preload: Table = package.get("preload")?;
    let kkc_loader: Function = preload.get("kkc")?;
    let kkc: Table = kkc_loader.call(())?;
    kkc.set(
        "register_action_plugin",
        lua.create_function(move |lua, table: Table| {
            let discover: Option<Function> = table.get("discover").ok();
            let run: Option<Function> = table.get("run").ok();
            if discover.is_none() || run.is_none() {
                return Err(mlua::Error::external(
                    "Action plugin does not define discover() and run()",
                ));
            }
            handles.borrow_mut().push(lua.create_registry_value(table)?);
            Ok(())
        })?,
    )?;
    preload.set("kkc", lua.create_function(move |_, ()| Ok(kkc.clone()))?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines_to_text(lines: &[Vec<ViewerSpan>]) -> String {
        lines
            .iter()
            .flat_map(|line| line.iter().map(|span| span.text.as_str()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn store_index_lists_plugins_and_applications() {
        let index_path =
            std::env::temp_dir().join(format!("kkc-store-index-{}.json", std::process::id()));
        fs::write(
            &index_path,
            r#"{
  "schema_version": 1,
  "generated_at": "2026-04-29T18:22:35.350023+00:00",
  "plugins_count": 1,
    "applications_count": 2,
  "plugins": [
    {
      "id": "json-viewer",
      "name": "JSON Viewer",
      "version": "1.2.0",
      "type": "viewer",
      "description": "JSON viewer",
      "location": { "kind": "local", "path": "plugins/json/assets" }
    }
  ],
  "applications": [
    {
      "id": "bat",
      "name": "bat",
      "version": "0.25.0",
      "description": "Syntax-highlighting text viewer.",
      "category": "viewer",
      "type": "external_viewer",
      "mime_types": ["text/plain", "application/json"],
            "wait_for_key_after_exit": true,
            "args": ["--style=plain", "%f"],
      "install": [
        { "os": ["macos", "linux", "windows"], "method": "cargo", "crate": "bat", "bin": "bat" }
      ]
        },
        {
            "id": "hexyl",
            "name": "hexyl",
            "version": "0.14.0",
            "description": "Hex viewer",
            "category": "viewer",
            "type": "external_viewer",
            "mime_types": ["application/octet-stream"],
            "args": "--plain \"%f\" --name '%n'",
            "install": [
                { "os": ["macos", "linux", "windows"], "method": "cargo", "crate": "hexyl", "bin": "hexyl" }
            ]
    }
  ],
  "tag": "0.0.8"
}"#,
        )
        .expect("write store index");

        let (items, info) = list_store_plugins_with_info(&index_path).expect("read index");
        assert_eq!(info.plugins_count, Some(1));
                assert_eq!(info.applications_count, Some(2));
                assert_eq!(items.len(), 3);

        let app = items.iter().find(|item| item.id == "bat").expect("bat app");
        assert!(matches!(app.item_kind, StoreItemKind::Application));
        assert_eq!(app.plugin_type, "external_viewer");
        assert_eq!(
            app.install_method.as_deref(),
            Some("cargo [macos/linux/windows]  crate bat  bin bat")
        );
        assert_eq!(app.install_bin.as_deref(), Some("bat"));
        assert_eq!(app.mime_types, vec!["text/plain", "application/json"]);
        assert!(app.wait_for_key_after_exit);
        assert_eq!(app.launch_args.as_deref(), Some("'--style=plain' '%f'"));
        assert_eq!(
            app.install_methods,
            vec!["cargo [macos/linux/windows]  crate bat  bin bat"]
        );

        let hexyl = items
            .iter()
            .find(|item| item.id == "hexyl")
            .expect("hexyl app");
        assert!(matches!(hexyl.item_kind, StoreItemKind::Application));
        assert_eq!(
            hexyl.launch_args.as_deref(),
            Some("--plain \"%f\" --name '%n'")
        );

        let plugin = items
            .iter()
            .find(|item| item.id == "json-viewer")
            .expect("json plugin");
        assert!(matches!(plugin.item_kind, StoreItemKind::Plugin));
        assert_eq!(plugin.plugin_type, "viewer");

        let _ = fs::remove_file(&index_path);
    }

    #[test]
    fn plugin_remove_only_allows_non_bundled_direct_children() {
        let root = Path::new("/tmp/kkc-store");

        assert!(!plugin_can_remove(root, root));
        assert!(!plugin_can_remove(&root.join("csv_viewer"), root));
        assert!(plugin_can_remove(&root.join("store_plugin"), root));
        assert!(!plugin_can_remove(
            &root.join("nested").join("plugin"),
            root
        ));
        assert!(!plugin_can_remove(Path::new("/tmp/other/plugin"), root));
    }

    #[test]
    fn bundled_pdf_plugin_registers() {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("plugins")
            .join("pdf_file")
            .join("plugin.lua");

        let plugins = inspect_plugin(&script).expect("plugin should load");

        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "pdf_file");
        assert_eq!(plugins[0].version, "1.0.0");
        assert_eq!(plugins[0].mime_types, vec!["application/pdf"]);
    }

    #[test]
    fn bundled_text_syntax_viewer_plugin_registers() {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("plugins")
            .join("text_syntax")
            .join("plugin.lua");

        let plugins = inspect_viewer_plugin(&script).expect("viewer plugin should load");

        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "text_syntax");
        assert_eq!(plugins[0].version, "1.0.0");
        assert_eq!(plugins[0].modes, vec!["text"]);
    }

    #[test]
    fn bundled_json_viewer_plugin_registers() {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("plugins")
            .join("json_viewer")
            .join("plugin.lua");

        let plugins = inspect_viewer_plugin(&script).expect("viewer plugin should load");

        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "json_viewer");
        assert_eq!(plugins[0].version, "1.0.0");
        assert_eq!(plugins[0].modes, vec!["text"]);
        assert_eq!(plugins[0].mime_types, vec!["application/json"]);
    }

    #[test]
    fn bundled_xml_viewer_plugin_registers() {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("plugins")
            .join("xml_viewer")
            .join("plugin.lua");

        let plugins = inspect_viewer_plugin(&script).expect("viewer plugin should load");

        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "xml_viewer");
        assert_eq!(plugins[0].version, "1.0.0");
        assert_eq!(plugins[0].modes, vec!["text"]);
        assert_eq!(
            plugins[0].mime_types,
            vec![
                "application/xml",
                "text/xml",
                "application/rss+xml",
                "application/atom+xml",
                "application/x-plist",
                "image/svg+xml"
            ]
        );
    }

    #[test]
    fn bundled_html_viewer_plugin_registers() {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("plugins")
            .join("html_viewer")
            .join("plugin.lua");

        let plugins = inspect_viewer_plugin(&script).expect("viewer plugin should load");

        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "html_viewer");
        assert_eq!(plugins[0].version, "2.0.0");
        assert_eq!(plugins[0].modes, vec!["text"]);
        assert_eq!(
            plugins[0].mime_types,
            vec!["text/html", "application/xhtml+xml"]
        );
    }

    #[test]
    fn bundled_eml_viewer_plugin_registers() {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("plugins")
            .join("eml_viewer")
            .join("plugin.lua");

        let plugins = inspect_viewer_plugin(&script).expect("viewer plugin should load");

        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "eml_viewer");
        assert_eq!(plugins[0].version, "1.0.0");
        assert_eq!(plugins[0].modes, vec!["text"]);
        assert_eq!(
            plugins[0].mime_types,
            vec!["message/rfc822", "application/mbox"]
        );
    }

    #[test]
    fn bundled_markdown_viewer_plugin_registers() {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("plugins")
            .join("markdown_viewer")
            .join("plugin.lua");

        let plugins = inspect_viewer_plugin(&script).expect("viewer plugin should load");

        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "markdown_viewer");
        assert_eq!(plugins[0].version, "1.0.0");
        assert_eq!(plugins[0].modes, vec!["text"]);
        assert_eq!(plugins[0].mime_types, vec!["text/markdown"]);
    }

    #[test]
    fn bundled_git_action_plugin_registers() {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("plugins")
            .join("git_action")
            .join("plugin.lua");

        let plugins = inspect_action_plugin(&script).expect("action plugin should load");

        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "git_action");
        assert_eq!(plugins[0].version, "1.0.0");
    }

    #[test]
    fn bundled_csv_viewer_supports_nowrap_state() {
        let csv_path =
            std::env::temp_dir().join(format!("kkc-csv-viewer-{}.csv", std::process::id()));
        fs::write(
            &csv_path,
            "Name;Description\nAlpha;this is a very long column value that should stay complete in nowrap\n",
        )
        .expect("write csv");

        let script_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("plugins")
            .join("csv_viewer")
            .join("plugin.lua");
        let plugin = ViewerPlugin {
            name: "csv_viewer".into(),
            version: "1.0.0".into(),
            description: String::new(),
            plugin_dir: script_path.parent().expect("plugin dir").to_path_buf(),
            script_path,
            modes: vec!["text".into()],
            mime_types: vec!["text/csv".into()],
            extensions: vec!["csv".into()],
        };

        let default_rendered = plugin
            .render_document(&csv_path, "text", &HashMap::new(), 80)
            .expect("csv viewer should render")
            .expect("csv viewer should return lines");
        let default_text = lines_to_text(&default_rendered);
        assert!(default_text.contains("wrap: "));
        assert!(default_text.contains("on"));
        assert_eq!(
            default_rendered[2][0].text.as_str(),
            " │ ",
            "CSV header gutter should be blank"
        );
        assert_eq!(default_rendered[4][0].text.as_str(), "1│ ");

        let (_, state) = plugin
            .handle_key(&csv_path, "text", "f2", &HashMap::new())
            .expect("csv viewer should handle key")
            .expect("csv viewer should return state");
        assert_eq!(state.get("wrap").map(String::as_str), Some("0"));

        let nowrap_rendered = plugin
            .render_document(&csv_path, "text", &state, 80)
            .expect("csv viewer should render nowrap")
            .expect("csv viewer should return nowrap lines");
        let nowrap_text = lines_to_text(&nowrap_rendered);
        assert!(nowrap_text.contains("wrap: "));
        assert!(nowrap_text.contains("off +0"));
        assert!(nowrap_text.contains("this is a very long column value that should stay complete"));

        let (_, scrolled_state) = plugin
            .handle_key(&csv_path, "text", "right", &state)
            .expect("csv viewer should handle scroll")
            .expect("csv viewer should return scrolled state");
        assert_eq!(scrolled_state.get("hscroll").map(String::as_str), Some("8"));

        let _ = fs::remove_file(&csv_path);
    }

    #[test]
    fn bundled_html_viewer_renders_document() {
        let html_path =
            std::env::temp_dir().join(format!("kkc-html-viewer-{}.html", std::process::id()));
        fs::write(
            &html_path,
            r##"<html><body><h1>Title</h1><p>Hello <a href="#x">link</a></p><table><tr><th>Name</th><th>Score</th></tr><tr><td>Alice</td><td>42</td></tr></table></body></html>"##,
        )
        .expect("write html");

        let script_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("plugins")
            .join("html_viewer")
            .join("plugin.lua");
        let plugin = ViewerPlugin {
            name: "html_viewer".into(),
            version: "1.0.0".into(),
            description: String::new(),
            plugin_dir: script_path.parent().expect("plugin dir").to_path_buf(),
            script_path,
            modes: vec!["text".into()],
            mime_types: vec!["text/html".into(), "application/xhtml+xml".into()],
            extensions: vec!["html".into(), "htm".into(), "xhtml".into()],
        };

        let rendered = plugin
            .render_document(&html_path, "text", &HashMap::new(), 120)
            .expect("html viewer should render")
            .expect("html viewer should return lines");
        let text = lines_to_text(&rendered);
        assert!(text.contains("kkc-html-viewer"));
        assert!(text.contains("Title"));
        assert!(text.contains("Hello"));
        assert!(text.contains("link"));
        assert!(text.contains("Name"));
        assert!(text.contains("Score"));
        assert!(text.contains("Alice"));
        assert!(text.contains("│"));

        let _ = fs::remove_file(&html_path);
    }

    #[test]
    fn bundled_eml_viewer_renders_message() {
        let eml_path =
            std::env::temp_dir().join(format!("kkc-eml-viewer-{}.eml", std::process::id()));
        fs::write(
            &eml_path,
            "From: sender@example.com\r\nTo: you@example.com\r\nSubject: Test message\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nHello body\r\n",
        )
        .expect("write eml");

        let script_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("plugins")
            .join("eml_viewer")
            .join("plugin.lua");
        let plugin = ViewerPlugin {
            name: "eml_viewer".into(),
            version: "1.0.0".into(),
            description: String::new(),
            plugin_dir: script_path.parent().expect("plugin dir").to_path_buf(),
            script_path,
            modes: vec!["text".into()],
            mime_types: vec!["message/rfc822".into(), "application/mbox".into()],
            extensions: vec!["eml".into(), "mbox".into()],
        };

        let rendered = plugin
            .render_document(&eml_path, "text", &HashMap::new(), 120)
            .expect("eml viewer should render")
            .expect("eml viewer should return lines");
        let text = lines_to_text(&rendered);
        assert!(text.contains("Message"));
        assert!(text.contains("Test message"));
        assert!(text.contains("Hello body"));

        let _ = fs::remove_file(&eml_path);
    }

    #[test]
    fn bundled_markdown_viewer_renders_document() {
        let md_path =
            std::env::temp_dir().join(format!("kkc-markdown-viewer-{}.md", std::process::id()));
        fs::write(
            &md_path,
            "# Title\n\nHello **bold** and [link](https://example.com).\n\n- Item\n\n| Name | Score |\n| --- | ---: |\n| Alice | 42 |\n\n```rust\nfn main() {}\n```\n",
        )
        .expect("write markdown");

        let script_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("plugins")
            .join("markdown_viewer")
            .join("plugin.lua");
        let plugin = ViewerPlugin {
            name: "markdown_viewer".into(),
            version: "1.0.0".into(),
            description: String::new(),
            plugin_dir: script_path.parent().expect("plugin dir").to_path_buf(),
            script_path,
            modes: vec!["text".into()],
            mime_types: vec!["text/markdown".into()],
            extensions: vec!["md".into()],
        };

        let rendered = plugin
            .render_document(&md_path, "text", &HashMap::new(), 120)
            .expect("markdown viewer should render")
            .expect("markdown viewer should return lines");
        let text = lines_to_text(&rendered);
        assert!(text.contains("Markdown"));
        assert!(text.contains("Title"));
        assert!(text.contains("bold"));
        assert!(text.contains("https://example.com"));
        assert!(text.contains("Name"));
        assert!(text.contains("Score"));
        assert!(text.contains("Alice"));
        assert!(text.contains("│"));
        assert!(text.contains("fn main"));

        let _ = fs::remove_file(&md_path);
    }

    #[test]
    fn bundled_markdown_viewer_wraps_to_document_width() {
        let md_path = std::env::temp_dir().join(format!(
            "kkc-markdown-viewer-wrap-{}.md",
            std::process::id()
        ));
        fs::write(
            &md_path,
            "# A heading that should wrap when the panel is narrow\n\nThis paragraph contains **bold words** and a [link](https://example.com) that should wrap cleanly across several terminal rows.\n\n- A list item with enough text to wrap and align continuation lines under the item text\n",
        )
        .expect("write markdown");

        let script_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("plugins")
            .join("markdown_viewer")
            .join("plugin.lua");
        let plugin = ViewerPlugin {
            name: "markdown_viewer".into(),
            version: "1.0.0".into(),
            description: String::new(),
            plugin_dir: script_path.parent().expect("plugin dir").to_path_buf(),
            script_path,
            modes: vec!["text".into()],
            mime_types: vec!["text/markdown".into()],
            extensions: vec!["md".into()],
        };

        let rendered = plugin
            .render_document(&md_path, "text", &HashMap::new(), 32)
            .expect("markdown viewer should render")
            .expect("markdown viewer should return lines");
        let rendered_text: Vec<String> = rendered
            .iter()
            .map(|line| line.iter().map(|span| span.text.as_str()).collect())
            .collect();

        assert!(
            rendered_text
                .iter()
                .any(|line| line == "# A heading that should wrap")
        );
        assert!(
            rendered_text
                .iter()
                .any(|line| line == "  when the panel is narrow")
        );
        assert!(
            rendered_text
                .iter()
                .any(|line| line.starts_with("• A list item"))
        );
        assert!(
            rendered_text
                .iter()
                .any(|line| line.starts_with("  ") && line.contains("continuation"))
        );
        let too_wide = rendered_text
            .iter()
            .filter(|line| text_len_for_test(line) > 32)
            .cloned()
            .collect::<Vec<_>>();
        assert!(too_wide.is_empty(), "too-wide lines: {too_wide:?}");

        let _ = fs::remove_file(&md_path);
    }

    fn text_len_for_test(text: &str) -> usize {
        text.chars().count()
    }

    #[test]
    fn bundled_json_viewer_renders_pretty_and_tree() {
        let json_path =
            std::env::temp_dir().join(format!("kkc-json-viewer-{}.json", std::process::id()));
        fs::write(
            &json_path,
            r#"{"name":"KKC","items":[1,true,null],"nested":{"ok":false}}"#,
        )
        .expect("write json");

        let script_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("plugins")
            .join("json_viewer")
            .join("plugin.lua");
        let plugin = ViewerPlugin {
            name: "json_viewer".into(),
            version: "1.0.0".into(),
            description: String::new(),
            plugin_dir: script_path.parent().expect("plugin dir").to_path_buf(),
            script_path,
            modes: vec!["text".into()],
            mime_types: vec!["application/json".into()],
            extensions: vec!["json".into(), "geojson".into()],
        };

        let pretty = plugin
            .render_document(&json_path, "text", &HashMap::new(), 120)
            .expect("json viewer should render")
            .expect("json viewer should return lines");
        let pretty_text = lines_to_text(&pretty);
        assert!(pretty_text.contains("JSON"));
        assert!(pretty_text.contains("name"));
        assert!(pretty_text.contains("items"));

        let mut state = HashMap::new();
        state.insert("view".into(), "tree".into());
        let tree = plugin
            .render_document(&json_path, "text", &state, 120)
            .expect("json viewer tree should render")
            .expect("json viewer tree should return lines");
        let tree_text = lines_to_text(&tree);
        assert!(tree_text.contains("$.items[2]"));
        assert!(tree_text.contains("$.nested.ok"));

        let _ = fs::remove_file(&json_path);
    }

    #[test]
    fn bundled_xml_viewer_renders_document() {
        let xml_path =
            std::env::temp_dir().join(format!("kkc-xml-viewer-{}.xml", std::process::id()));
        fs::write(
            &xml_path,
            r#"<?xml version="1.0"?><catalog><book id="bk101"><title>XML Guide</title><!-- note --><data><![CDATA[a < b]]></data></book></catalog>"#,
        )
        .expect("write xml");

        let script_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("plugins")
            .join("xml_viewer")
            .join("plugin.lua");
        let plugin = ViewerPlugin {
            name: "xml_viewer".into(),
            version: "1.0.0".into(),
            description: String::new(),
            plugin_dir: script_path.parent().expect("plugin dir").to_path_buf(),
            script_path,
            modes: vec!["text".into()],
            mime_types: vec![
                "application/xml".into(),
                "text/xml".into(),
                "application/rss+xml".into(),
                "application/atom+xml".into(),
                "application/x-plist".into(),
                "image/svg+xml".into(),
            ],
            extensions: vec![
                "xml".into(),
                "xsd".into(),
                "xsl".into(),
                "xslt".into(),
                "svg".into(),
                "rss".into(),
                "atom".into(),
                "plist".into(),
            ],
        };

        let rendered = plugin
            .render_document(&xml_path, "text", &HashMap::new(), 120)
            .expect("xml viewer should render")
            .expect("xml viewer should return lines");
        let text = lines_to_text(&rendered);
        assert!(text.contains("XML"));
        assert!(text.contains("catalog"));
        assert!(text.contains("book"));
        assert!(text.contains("id"));
        assert!(text.contains("bk101"));
        assert!(text.contains("XML Guide"));
        assert!(text.contains("note"));
        assert!(text.contains("CDATA"));

        let (_, state) = plugin
            .handle_key(&xml_path, "text", "f2", &HashMap::new())
            .expect("xml viewer should handle key")
            .expect("xml viewer should return state");
        assert_eq!(state.get("wrap").map(String::as_str), Some("0"));

        let _ = fs::remove_file(&xml_path);
    }

    #[test]
    fn bundled_text_syntax_highlights_rust_keywords() {
        let script_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("plugins")
            .join("text_syntax")
            .join("plugin.lua");
        let plugin_dir = script_path.parent().expect("plugin dir").to_path_buf();
        let plugin = ViewerPlugin {
            name: "text_syntax".into(),
            version: "1.0.0".into(),
            description: String::new(),
            script_path,
            plugin_dir,
            modes: vec!["text".into()],
            mime_types: Vec::new(),
            extensions: Vec::new(),
        };
        let lines = vec!["fn main() { let answer = 42; }".to_string()];

        let highlighted = plugin
            .highlight_lines(Path::new("main.rs"), "text", &lines)
            .expect("highlight should run")
            .expect("highlight should return spans");

        assert_eq!(highlighted.len(), 1);
        assert!(
            highlighted[0]
                .iter()
                .any(|span| span.text == "fn" && span.fg == "yellow" && span.bold)
        );
        assert!(
            highlighted[0]
                .iter()
                .any(|span| span.text == "42" && span.fg == "cyan")
        );
        assert!(
            highlighted[0]
                .iter()
                .all(|span| span.bg.as_deref() == Some("black"))
        );
    }

    #[test]
    fn lua_plugins_can_write_debug_log() {
        let root = std::env::temp_dir().join(format!(
            "kkc-debug-log-plugin-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&root).expect("temp dir");
        let script_path = root.join("plugin.lua");
        fs::write(
            &script_path,
            r#"local kkc = require("kkc")
kkc.debug_log("viewer plugin registered")
kkc.register_viewer_plugin({
    name = "debug_log_test",
    modes = { "text" },
    render_line = function(_, _, line)
        return { { text = line, fg = "white" } }
    end,
})
"#,
        )
        .expect("write plugin");

        let plugins = inspect_viewer_plugin(&script_path).expect("viewer plugin should load");

        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "debug_log_test");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn kkplug_bundle_extracts_plugin_root() {
        let root = std::env::temp_dir().join(format!(
            "kkc-kkplug-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let plugins_dir = root.join("plugins");
        fs::create_dir_all(&plugins_dir).expect("temp plugins dir");
        let bundle = root.join("sample.kkplug");
        {
            let file = fs::File::create(&bundle).expect("bundle file");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file("wrapped/plugin.lua", options)
                .expect("start plugin");
            std::io::Write::write_all(
                &mut zip,
                br#"local kkc = require("kkc")
kkc.register_viewer_plugin({ name = "wrapped", modes = { "text" }, render_line = function(_, _, line) return { { text = line, fg = "white" } } end })
"#,
            )
            .expect("write plugin");
            zip.start_file("wrapped/extra.lua", options)
                .expect("start extra");
            std::io::Write::write_all(&mut zip, b"return {}").expect("write extra");
            zip.finish().expect("finish zip");
        }

        let installed = extract_plugin_bundle(&bundle, &plugins_dir).expect("install bundle");

        assert_eq!(installed, plugins_dir.join("sample"));
        assert!(installed.join("plugin.lua").is_file());
        assert!(installed.join("extra.lua").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn kkplug_bundle_cannot_replace_bundled_plugin() {
        let root = std::env::temp_dir().join(format!(
            "kkc-kkplug-bundled-conflict-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let plugins_dir = root.join("plugins");
        fs::create_dir_all(&plugins_dir).expect("temp plugins dir");
        let bundle = root.join("csv_viewer.kkplug");
        {
            let file = fs::File::create(&bundle).expect("bundle file");
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file("plugin.lua", options).expect("start plugin");
            std::io::Write::write_all(
                &mut zip,
                br#"local kkc = require("kkc")
kkc.register_viewer_plugin({ name = "csv_viewer", modes = { "text" }, render_line = function(_, _, line) return { { text = line, fg = "white" } } end })
"#,
            )
            .expect("write plugin");
            zip.finish().expect("finish zip");
        }

        let err = extract_plugin_bundle(&bundle, &plugins_dir)
            .expect_err("bundle must not replace bundled plugin");
        assert!(err.to_string().contains("conflicts with bundled plugin"));
        assert!(!plugins_dir.join("csv_viewer").exists());
        let _ = fs::remove_dir_all(root);
    }
}
