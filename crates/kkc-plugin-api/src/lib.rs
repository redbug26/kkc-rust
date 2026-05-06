use abi_stable::{
    StableAbi,
    library::RootModule,
    package_version_strings,
    sabi_types::VersionStrings,
    std_types::{RResult, RStr, RString, RVec},
};

pub const KKC_REMOTE_PLUGIN_API_VERSION: u32 = 3;
pub const KKC_VIEWER_PLUGIN_API_VERSION: u32 = 2;

pub type RemotePluginResult<T> = RResult<T, RString>;
pub type ViewerPluginResult<T> = RResult<T, RString>;

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
pub struct ViewerPluginMetadata {
    pub id: RString,
    pub name: RString,
    pub version: RString,
    pub description: RString,
    pub modes: RVec<RString>,
    pub mime_types: RVec<RString>,
    pub extensions: RVec<RString>,
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct ViewerSpan {
    pub text: RString,
    pub fg: RString,
    pub bg: RString,
    pub bold: bool,
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct ViewerLine {
    pub spans: RVec<ViewerSpan>,
}

#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct ViewerHandleKeyResult {
    pub consumed: bool,
    pub state_json: RString,
}

/// Image data for viewer plugins (PNG or RGB raw bytes, base64 encoded)
#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct ViewerImage {
    /// Base64-encoded PNG or raw RGB data
    pub data: RString,
    /// Format: "png" or "rgb"
    pub format: RString,
    /// Image width in pixels
    pub width: u32,
    /// Image height in pixels
    pub height: u32,
}

/// Result from render_document_image: image + optional text overlay lines
#[repr(C)]
#[derive(Debug, Clone, StableAbi)]
pub struct ViewerDocumentImage {
    /// The rendered image
    pub image: ViewerImage,
    /// Optional text lines to overlay/show below the image
    pub overlay_lines: RVec<ViewerLine>,
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

#[repr(C)]
#[derive(StableAbi)]
#[sabi(kind(Prefix(prefix_ref = ViewerPluginModRef)))]
#[sabi(missing_field(panic))]
pub struct ViewerPluginMod {
    pub api_version: extern "C" fn() -> u32,
    pub metadata: extern "C" fn() -> ViewerPluginMetadata,
    pub render_document: extern "C" fn(
        path: RStr<'_>,
        mode: RStr<'_>,
        state_json: RStr<'_>,
        width: u64,
    ) -> ViewerPluginResult<RVec<ViewerLine>>,
    pub render_document_image: extern "C" fn(
        path: RStr<'_>,
        mode: RStr<'_>,
        state_json: RStr<'_>,
        width: u64,
        height: u64,
    ) -> ViewerPluginResult<ViewerDocumentImage>,
    #[sabi(last_prefix_field)]
    pub handle_key: extern "C" fn(
        path: RStr<'_>,
        mode: RStr<'_>,
        key: RStr<'_>,
        state_json: RStr<'_>,
    ) -> ViewerPluginResult<ViewerHandleKeyResult>,
}

impl RootModule for RemotePluginModRef {
    abi_stable::declare_root_module_statics! {RemotePluginModRef}
    const BASE_NAME: &'static str = "kkc_remote_plugin";
    const NAME: &'static str = "kkc_remote_plugin";
    const VERSION_STRINGS: VersionStrings = package_version_strings!();
}

impl RootModule for ViewerPluginModRef {
    abi_stable::declare_root_module_statics! {ViewerPluginModRef}
    const BASE_NAME: &'static str = "kkc_viewer_plugin";
    const NAME: &'static str = "kkc_viewer_plugin";
    const VERSION_STRINGS: VersionStrings = package_version_strings!();
}
