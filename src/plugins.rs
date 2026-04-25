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

static PLUGINS: OnceLock<PluginRegistry> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct PluginRegistry {
    archive_plugins: Vec<ArchivePlugin>,
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
struct RegisteredPlugin {
    name: String,
    description: String,
    extensions: Vec<String>,
    can_add_files: bool,
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

fn load_plugins() -> Result<PluginRegistry> {
    let plugins_dir = ensure_plugins_dir()?;
    install_bundled_plugins(&plugins_dir)?;

    let mut archive_plugins = Vec::new();
    for script_path in plugin_scripts(&plugins_dir)? {
        let registered = inspect_plugin(&script_path)
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
    }

    Ok(PluginRegistry { archive_plugins })
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
        self.archive_plugins
            .iter()
            .map(|plugin| PluginInfo {
                name: plugin.name.clone(),
                kind: "Archive".into(),
                description: plugin.description.clone(),
                extensions: plugin.extensions.clone(),
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
}
