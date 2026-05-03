use abi_stable::{
    StableAbi,
    library::RootModule,
    package_version_strings,
    sabi_types::VersionStrings,
    std_types::{RResult, RStr, RString, RVec},
};

pub const KKC_REMOTE_PLUGIN_API_VERSION: u32 = 3;

pub type RemotePluginResult<T> = RResult<T, RString>;

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct RemoteConfigField {
    pub key: RString,
    pub label: RString,
    pub secret: bool,
    pub required: bool,
    pub default_value: RString,
}

impl RemoteConfigField {
    pub fn new(
        key: impl Into<RString>,
        label: impl Into<RString>,
        secret: bool,
        required: bool,
        default_value: impl Into<RString>,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            secret,
            required,
            default_value: default_value.into(),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct RemotePluginMetadata {
    pub id: RString,
    pub name: RString,
    pub version: RString,
    pub description: RString,
    pub scheme: RString,
    pub fields: RVec<RemoteConfigField>,
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct RemoteEntry {
    pub name: RString,
    pub path: RString,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub modified_unix: i64,
    pub mode: u32,
}

#[repr(C)]
#[derive(StableAbi)]
#[sabi(kind(Prefix(prefix_ref = RemotePluginModRef)))]
#[sabi(missing_field(panic))]
pub struct RemotePluginMod {
    pub api_version: extern "C" fn() -> u32,
    pub metadata: extern "C" fn() -> RemotePluginMetadata,
    pub normalize_cwd:
        extern "C" fn(config_json: RStr<'_>, cwd: RStr<'_>) -> RemotePluginResult<RString>,
    pub list_dir: extern "C" fn(
        config_json: RStr<'_>,
        cwd: RStr<'_>,
        show_hidden: bool,
    ) -> RemotePluginResult<RVec<RemoteEntry>>,
    pub download_into_dir: extern "C" fn(
        config_json: RStr<'_>,
        remote_path: RStr<'_>,
        local_dir: RStr<'_>,
        recursive: bool,
    ) -> RemotePluginResult<RString>,
    pub upload_into_dir: extern "C" fn(
        config_json: RStr<'_>,
        local_path: RStr<'_>,
        remote_dir: RStr<'_>,
        recursive: bool,
    ) -> RemotePluginResult<RString>,
    pub delete_path: extern "C" fn(
        config_json: RStr<'_>,
        remote_path: RStr<'_>,
        is_dir: bool,
    ) -> RemotePluginResult<()>,
    pub set_debug_log: extern "C" fn(callback: usize),
    pub make_dir:
        extern "C" fn(config_json: RStr<'_>, remote_path: RStr<'_>) -> RemotePluginResult<()>,
    pub auth_start: extern "C" fn(config_json: RStr<'_>) -> RemotePluginResult<RString>,
    #[sabi(last_prefix_field)]
    pub auth_complete: extern "C" fn(
        config_json: RStr<'_>,
        auth_session_json: RStr<'_>,
        input: RStr<'_>,
    ) -> RemotePluginResult<RString>,
}

impl RootModule for RemotePluginModRef {
    abi_stable::declare_root_module_statics! {RemotePluginModRef}
    const BASE_NAME: &'static str = "kkc_remote_plugin";
    const NAME: &'static str = "kkc_remote_plugin";
    const VERSION_STRINGS: VersionStrings = package_version_strings!();
}
