use anyhow::{Context, Result, anyhow, bail};
use mlua::{Function, Lua, Table, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Seek};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{OnceLock, RwLock};
use zip::ZipArchive;

const BUNDLED_DSK_LUA: &str = include_str!("../assets/plugins/amstrad_dsk/dsk.lua");
const BUNDLED_DSK_LUA_LICENSE: &str = include_str!("../assets/plugins/amstrad_dsk/LICENSE.dsk-lua");
const BUNDLED_AMSTRAD_DSK_PLUGIN: &str = include_str!("../assets/plugins/amstrad_dsk/plugin.lua");
const BUNDLED_COMMODORE_D64_PLUGIN: &str =
    include_str!("../assets/plugins/commodore_d64/plugin.lua");
const BUNDLED_LHA_LZH_PLUGIN: &str = include_str!("../assets/plugins/lha_lzh/plugin.lua");
const BUNDLED_PDF_FILE_PLUGIN: &str = include_str!("../assets/plugins/pdf_file/plugin.lua");
const BUNDLED_HTML_VIEWER_PLUGIN: &str = include_str!("../assets/plugins/html_viewer/plugin.lua");
const BUNDLED_EML_VIEWER_PLUGIN: &str = include_str!("../assets/plugins/eml_viewer/plugin.lua");
const BUNDLED_JSON_VIEWER_PLUGIN: &str = include_str!("../assets/plugins/json_viewer/plugin.lua");
const BUNDLED_CSV_VIEWER_PLUGIN: &str = include_str!("../assets/plugins/csv_viewer/plugin.lua");
const BUNDLED_MARKDOWN_VIEWER_PLUGIN: &str =
    include_str!("../assets/plugins/markdown_viewer/plugin.lua");
const BUNDLED_TEXT_SYNTAX_PLUGIN: &str = include_str!("../assets/plugins/text_syntax/plugin.lua");

static PLUGINS: OnceLock<RwLock<PluginRegistry>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct PluginRegistry {
    archive_plugins: Vec<ArchivePlugin>,
    viewer_plugins: Vec<ViewerPlugin>,
}

#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub name: String,
    pub kind: String,
    pub description: String,
    pub extensions: Vec<String>,
}

#[derive(Debug, Clone)]
struct ArchivePlugin {
    name: String,
    description: String,
    script_path: PathBuf,
    plugin_dir: PathBuf,
    extensions: Vec<String>,
    can_add_files: bool,
}

#[derive(Debug, Clone)]
struct ViewerPlugin {
    name: String,
    description: String,
    script_path: PathBuf,
    plugin_dir: PathBuf,
    modes: Vec<String>,
    extensions: Vec<String>,
}

#[derive(Debug, Clone)]
struct RegisteredPlugin {
    name: String,
    description: String,
    extensions: Vec<String>,
    can_add_files: bool,
}

#[derive(Debug, Clone)]
struct RegisteredViewerPlugin {
    name: String,
    description: String,
    modes: Vec<String>,
    extensions: Vec<String>,
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
        return Ok(());
    }

    let registry = load_plugins()?;
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
    PLUGINS
        .get()
        .and_then(|registry| registry.read().ok().map(|registry| registry.plugin_infos()))
        .unwrap_or_default()
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

fn load_plugins() -> Result<PluginRegistry> {
    let plugins_dir = ensure_plugins_dir()?;
    install_bundled_plugins(&plugins_dir)?;

    let mut archive_plugins = Vec::new();
    let mut viewer_plugins = Vec::new();
    for script_path in plugin_scripts(&plugins_dir)? {
        let (registered, registered_viewers) = inspect_plugins(&script_path)
            .with_context(|| format!("Loading plugin {}", script_path.display()))?;
        let plugin_dir = script_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| plugins_dir.clone());

        for plugin in registered {
            archive_plugins.push(ArchivePlugin {
                name: plugin.name,
                description: plugin.description,
                script_path: script_path.clone(),
                plugin_dir: plugin_dir.clone(),
                extensions: plugin.extensions,
                can_add_files: plugin.can_add_files,
            });
        }
        for plugin in registered_viewers {
            viewer_plugins.push(ViewerPlugin {
                name: plugin.name,
                description: plugin.description,
                script_path: script_path.clone(),
                plugin_dir: plugin_dir.clone(),
                modes: plugin.modes,
                extensions: plugin.extensions,
            });
        }
    }

    Ok(PluginRegistry {
        archive_plugins,
        viewer_plugins,
    })
}

fn ensure_plugins_dir() -> Result<PathBuf> {
    let plugins_dir = crate::config::data_dir()?.join("plugins");
    fs::create_dir_all(&plugins_dir)?;
    Ok(plugins_dir)
}

fn install_bundled_plugins(plugins_dir: &Path) -> Result<()> {
    let amstrad_dir = plugins_dir.join("amstrad_dsk");
    fs::create_dir_all(&amstrad_dir)?;

    write_bundled_file(&amstrad_dir.join("dsk.lua"), BUNDLED_DSK_LUA)?;
    write_bundled_file(
        &amstrad_dir.join("LICENSE.dsk-lua"),
        BUNDLED_DSK_LUA_LICENSE,
    )?;
    write_bundled_file(&amstrad_dir.join("plugin.lua"), BUNDLED_AMSTRAD_DSK_PLUGIN)?;

    let commodore_dir = plugins_dir.join("commodore_d64");
    fs::create_dir_all(&commodore_dir)?;
    write_bundled_file(
        &commodore_dir.join("plugin.lua"),
        BUNDLED_COMMODORE_D64_PLUGIN,
    )?;

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

    Ok(())
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
    let temp_dir = plugins_dir.join(format!(".install-{}-{}", plugin_name, std::process::id()));
    let install_dir = plugins_dir.join(&plugin_name);

    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir)
            .with_context(|| format!("Cleaning {}", temp_dir.display()))?;
    }
    fs::create_dir_all(&temp_dir).with_context(|| format!("Creating {}", temp_dir.display()))?;

    let mut has_plugin_lua = false;
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
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).with_context(|| format!("Creating {}", parent.display()))?;
        }
        let mut out =
            fs::File::create(&output).with_context(|| format!("Writing {}", output.display()))?;
        io::copy(&mut file, &mut out).with_context(|| format!("Extracting {}", file.name()))?;
    }

    if !has_plugin_lua {
        let _ = fs::remove_dir_all(&temp_dir);
        bail!("Plugin bundle does not contain plugin.lua at its root");
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
) -> Result<(Vec<RegisteredPlugin>, Vec<RegisteredViewerPlugin>)> {
    let lua = plugin_lua();
    let registered_archives = Rc::new(RefCell::new(Vec::new()));
    let registered_viewers = Rc::new(RefCell::new(Vec::new()));
    install_bindings(&lua, script_path.parent().unwrap_or(Path::new("")), {
        let registered_archives = Rc::clone(&registered_archives);
        move |plugin| registered_archives.borrow_mut().push(plugin)
    })?;
    install_viewer_registration(&lua, {
        let registered_viewers = Rc::clone(&registered_viewers);
        move |plugin| registered_viewers.borrow_mut().push(plugin)
    })?;

    let script = fs::read_to_string(script_path)?;
    lua.load(&script)
        .set_name(script_path.to_string_lossy())
        .exec()?;

    Ok((
        registered_archives.borrow().clone(),
        registered_viewers.borrow().clone(),
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
            let description: String = table.get("description").unwrap_or_else(|_| String::new());
            let extract: Option<Function> = table.get("extract")?;
            let add_files: Option<Function> = table.get("add_files").ok();
            if extract.is_none() {
                return Err(mlua::Error::external(format!(
                    "Plugin '{name}' does not define extract()"
                )));
            }

            let extensions = match table.get::<Option<Table>>("extensions")? {
                Some(values) => values
                    .sequence_values::<String>()
                    .map(|value| value.map(|ext| ext.trim_start_matches('.').to_ascii_lowercase()))
                    .collect::<mlua::Result<Vec<_>>>()?,
                None => Vec::new(),
            };

            on_register(RegisteredPlugin {
                name,
                description,
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

impl PluginRegistry {
    fn plugin_infos(&self) -> Vec<PluginInfo> {
        let mut plugins = self
            .archive_plugins
            .iter()
            .map(|plugin| PluginInfo {
                name: plugin.name.clone(),
                kind: "Archive".into(),
                description: plugin.description.clone(),
                extensions: plugin.extensions.clone(),
            })
            .collect::<Vec<_>>();
        plugins.extend(self.viewer_plugins.iter().map(|plugin| PluginInfo {
            name: plugin.name.clone(),
            kind: "Viewer".into(),
            description: plugin.description.clone(),
            extensions: if plugin.extensions.is_empty() {
                plugin.modes.clone()
            } else {
                plugin.extensions.clone()
            },
        }));
        plugins
    }

    fn viewer_plugin_infos(&self) -> Vec<PluginInfo> {
        self.viewer_plugins
            .iter()
            .map(|plugin| PluginInfo {
                name: plugin.name.clone(),
                kind: "Viewer".into(),
                description: plugin.description.clone(),
                extensions: if plugin.extensions.is_empty() {
                    plugin.modes.clone()
                } else {
                    plugin.extensions.clone()
                },
            })
            .collect()
    }

    fn default_viewer_plugin_for_path(&self, path: &Path) -> Option<&str> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        self.viewer_plugins
            .iter()
            .find(|plugin| plugin.extensions.iter().any(|value| value == &ext))
            .map(|plugin| plugin.name.as_str())
    }

    fn supports_archive(&self, path: &Path) -> bool {
        self.archive_plugins
            .iter()
            .any(|plugin| plugin.supports_path(path))
    }

    fn supports_add_files(&self, path: &Path) -> bool {
        self.archive_plugins
            .iter()
            .any(|plugin| plugin.supports_path(path) && plugin.can_add_files)
    }

    fn extract_archive(&self, path: &Path, destination: &Path) -> Result<bool> {
        let Some(plugin) = self
            .archive_plugins
            .iter()
            .find(|plugin| plugin.supports_path(path))
        else {
            return Ok(false);
        };

        plugin.extract(path, destination)?;
        Ok(true)
    }

    fn add_files(&self, path: &Path, sources: &[PathBuf]) -> Result<bool> {
        let Some(plugin) = self
            .archive_plugins
            .iter()
            .find(|plugin| plugin.supports_path(path) && plugin.can_add_files)
        else {
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
}

impl ArchivePlugin {
    fn supports_path(&self, path: &Path) -> bool {
        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else {
            return false;
        };
        let ext = ext.to_ascii_lowercase();
        self.extensions.iter().any(|candidate| candidate == &ext)
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
            let extensions = match table.get::<Option<Table>>("extensions")? {
                Some(values) => values
                    .sequence_values::<String>()
                    .map(|value| value.map(|ext| ext.trim_start_matches('.').to_ascii_lowercase()))
                    .collect::<mlua::Result<Vec<_>>>()?,
                None => Vec::new(),
            };
            on_register(RegisteredViewerPlugin {
                name,
                description,
                modes,
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
    fn bundled_pdf_plugin_registers() {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("plugins")
            .join("pdf_file")
            .join("plugin.lua");

        let plugins = inspect_plugin(&script).expect("plugin should load");

        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "pdf_file");
        assert_eq!(plugins[0].extensions, vec!["pdf"]);
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
        assert_eq!(plugins[0].modes, vec!["text"]);
        assert_eq!(plugins[0].extensions, vec!["json", "geojson"]);
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
        assert_eq!(plugins[0].modes, vec!["text"]);
        assert_eq!(plugins[0].extensions, vec!["html", "htm", "xhtml"]);
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
        assert_eq!(plugins[0].modes, vec!["text"]);
        assert_eq!(plugins[0].extensions, vec!["eml", "mbox"]);
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
        assert_eq!(plugins[0].modes, vec!["text"]);
        assert_eq!(
            plugins[0].extensions,
            vec!["md", "markdown", "mdown", "mkd"]
        );
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
            description: String::new(),
            plugin_dir: script_path.parent().expect("plugin dir").to_path_buf(),
            script_path,
            modes: vec!["text".into()],
            extensions: vec!["html".into(), "htm".into(), "xhtml".into()],
        };

        let rendered = plugin
            .render_document(&html_path, "text", &HashMap::new(), 120)
            .expect("html viewer should render")
            .expect("html viewer should return lines");
        let text = lines_to_text(&rendered);
        assert!(text.contains("HTML"));
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
            description: String::new(),
            plugin_dir: script_path.parent().expect("plugin dir").to_path_buf(),
            script_path,
            modes: vec!["text".into()],
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
            description: String::new(),
            plugin_dir: script_path.parent().expect("plugin dir").to_path_buf(),
            script_path,
            modes: vec!["text".into()],
            extensions: vec!["md".into(), "markdown".into(), "mdown".into(), "mkd".into()],
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
            description: String::new(),
            plugin_dir: script_path.parent().expect("plugin dir").to_path_buf(),
            script_path,
            modes: vec!["text".into()],
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
    fn bundled_amstrad_dsk_viewer_plugin_registers() {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("plugins")
            .join("amstrad_dsk")
            .join("plugin.lua");

        let plugins = inspect_viewer_plugin(&script).expect("viewer plugin should load");

        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "amstrad_dsk_directory");
        assert_eq!(plugins[0].modes, vec!["text"]);
    }

    #[test]
    fn bundled_commodore_d64_viewer_plugin_registers() {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("plugins")
            .join("commodore_d64")
            .join("plugin.lua");

        let plugins = inspect_viewer_plugin(&script).expect("viewer plugin should load");

        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "commodore_d64_directory");
        assert_eq!(plugins[0].modes, vec!["text"]);
    }

    #[test]
    fn commodore_d64_viewer_handles_petscii_and_del_entries() {
        fn d64_offset(track: usize, sector: usize) -> usize {
            const SECTORS_PER_TRACK: [usize; 35] = [
                21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 21, 19, 19, 19, 19,
                19, 19, 19, 18, 18, 18, 18, 18, 18, 17, 17, 17, 17, 17,
            ];
            (SECTORS_PER_TRACK[..track - 1].iter().sum::<usize>() + sector) * 256
        }

        let root = std::env::temp_dir().join(format!(
            "kkc-d64-view-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&root).expect("temp dir");
        let image_path = root.join("petscii.d64");
        let mut image = vec![0u8; 174_848];

        let bam = d64_offset(18, 0);
        image[bam] = 18;
        image[bam + 1] = 1;
        image[bam + 2] = b'A';
        image[bam + 144..bam + 148].copy_from_slice(&[0xd5, 0xc9, 0xca, 0xcb]);
        image[bam + 162..bam + 164].copy_from_slice(b"42");
        image[bam + 165..bam + 167].copy_from_slice(b"2A");

        let dir = d64_offset(18, 1);
        image[dir] = 0;
        image[dir + 1] = 255;
        image[dir + 2] = 0x80;
        image[dir + 5..dir + 8].copy_from_slice(&[0xc4, 0xc5, 0xcc]);
        image[dir + 8] = 0xb6;
        image[dir + 34] = 0x82;
        image[dir + 35] = 17;
        image[dir + 36] = 0;
        image[dir + 37..dir + 41].copy_from_slice(b"FILE");
        image[dir + 62] = 5;

        fs::write(&image_path, image).expect("write d64");

        let script_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join("plugins")
            .join("commodore_d64")
            .join("plugin.lua");
        let plugin = ViewerPlugin {
            name: "commodore_d64_directory".into(),
            description: String::new(),
            plugin_dir: script_path.parent().expect("plugin dir").to_path_buf(),
            script_path,
            modes: vec!["text".into()],
            extensions: Vec::new(),
        };

        let lines = plugin
            .render_document(&image_path, "text", &HashMap::new(), 120)
            .expect("viewer should render")
            .expect("viewer should return lines");
        let text = lines
            .iter()
            .flat_map(|line| line.iter().map(|span| span.text.as_str()))
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("\"UIJK"));
        assert!(text.contains("┤"));
        assert!(text.contains("\"DEL┤"));
        assert!(text.contains("DEL"));
        assert!(text.contains("\"FILE"));
        let _ = fs::remove_dir_all(root);
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
            description: String::new(),
            script_path,
            plugin_dir,
            modes: vec!["text".into()],
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
}
