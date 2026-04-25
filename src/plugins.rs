use anyhow::{Context, Result, anyhow, bail};
use mlua::{Function, Lua, Table};
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::OnceLock;

const BUNDLED_DSK_LUA: &str = include_str!("../assets/plugins/amstrad_dsk/dsk.lua");
const BUNDLED_DSK_LUA_LICENSE: &str = include_str!("../assets/plugins/amstrad_dsk/LICENSE.dsk-lua");
const BUNDLED_AMSTRAD_DSK_PLUGIN: &str = include_str!("../assets/plugins/amstrad_dsk/plugin.lua");
const BUNDLED_COMMODORE_D64_PLUGIN: &str =
    include_str!("../assets/plugins/commodore_d64/plugin.lua");
const BUNDLED_LHA_LZH_PLUGIN: &str = include_str!("../assets/plugins/lha_lzh/plugin.lua");
const BUNDLED_PDF_FILE_PLUGIN: &str = include_str!("../assets/plugins/pdf_file/plugin.lua");
const BUNDLED_TEXT_SYNTAX_PLUGIN: &str = include_str!("../assets/plugins/text_syntax/plugin.lua");

static PLUGINS: OnceLock<PluginRegistry> = OnceLock::new();

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
        .set(registry)
        .map_err(|_| anyhow!("Plugin registry already initialized"))?;
    Ok(())
}

pub fn supports_archive_navigation(path: &Path) -> bool {
    PLUGINS
        .get()
        .map(|registry| registry.supports_archive(path))
        .unwrap_or(false)
}

pub fn extract_archive_to_temp(path: &Path, destination: &Path) -> Result<bool> {
    let Some(registry) = PLUGINS.get() else {
        return Ok(false);
    };
    registry.extract_archive(path, destination)
}

pub fn supports_archive_add_files(path: &Path) -> bool {
    PLUGINS
        .get()
        .map(|registry| registry.supports_add_files(path))
        .unwrap_or(false)
}

pub fn add_files_to_archive(path: &Path, sources: &[PathBuf]) -> Result<bool> {
    let Some(registry) = PLUGINS.get() else {
        return Ok(false);
    };
    registry.add_files(path, sources)
}

pub fn plugins_dir() -> Result<PathBuf> {
    ensure_plugins_dir()
}

pub fn plugin_infos() -> Vec<PluginInfo> {
    PLUGINS
        .get()
        .map(PluginRegistry::plugin_infos)
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
            .highlight_viewer_lines(path, mode, plugin_name, lines)
            .ok()
            .flatten()
    })
}

pub fn viewer_plugin_infos() -> Vec<PluginInfo> {
    PLUGINS
        .get()
        .map(PluginRegistry::viewer_plugin_infos)
        .unwrap_or_default()
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
            extensions: plugin.modes.clone(),
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
                extensions: plugin.modes.clone(),
            })
            .collect()
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
                let render_line: Function = table.get("render_line")?;
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
}

fn lua_spans_to_viewer_spans(table: Table) -> Result<Vec<ViewerSpan>> {
    table
        .sequence_values::<Table>()
        .map(|span| {
            let span = span?;
            Ok(ViewerSpan {
                text: span.get("text")?,
                fg: span.get("fg").unwrap_or_else(|_| "white".into()),
                bg: span.get("bg").ok(),
                bold: span.get("bold").unwrap_or(false),
            })
        })
        .collect::<mlua::Result<Vec<_>>>()
        .map_err(Into::into)
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
            if render_line.is_none() {
                return Err(mlua::Error::external(format!(
                    "Viewer plugin '{name}' does not define render_line()"
                )));
            }
            let modes = match table.get::<Option<Table>>("modes")? {
                Some(values) => values
                    .sequence_values::<String>()
                    .collect::<mlua::Result<Vec<_>>>()?,
                None => Vec::new(),
            };
            on_register(RegisteredViewerPlugin {
                name,
                description,
                modes,
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
            if render_line.is_none() {
                return Err(mlua::Error::external(
                    "Viewer plugin does not define render_line()",
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
}
