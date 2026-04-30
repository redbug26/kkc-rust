use std::fs::Metadata;
use std::path::Path;

#[cfg(target_os = "macos")]
use std::ffi::CString;
#[cfg(target_os = "macos")]
use std::os::unix::ffi::OsStrExt;
#[cfg(target_os = "macos")]
use std::os::unix::fs::MetadataExt;

#[cfg(target_os = "macos")]
const CLOUD_XATTR_PREFIXES: &[&str] = &[
    "com.apple.fileprovider.",
    "com.apple.icloud.",
    "com.apple.decmpfs",
    "com.dropbox.",
];

#[cfg(target_os = "windows")]
const FILE_ATTRIBUTE_OFFLINE: u32 = 0x0000_1000;
#[cfg(target_os = "windows")]
const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x0004_0000;
#[cfg(target_os = "windows")]
const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;

pub fn is_cloud_only(path: &Path, metadata: &Metadata) -> bool {
    platform_is_cloud_only(path, metadata)
}

#[cfg(target_os = "windows")]
fn platform_is_cloud_only(_path: &Path, metadata: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    let attrs = metadata.file_attributes();
    attrs
        & (FILE_ATTRIBUTE_OFFLINE
            | FILE_ATTRIBUTE_RECALL_ON_OPEN
            | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
        != 0
}

#[cfg(target_os = "macos")]
fn platform_is_cloud_only(path: &Path, metadata: &Metadata) -> bool {
    if metadata.is_dir() {
        return false;
    }
    if !has_cloud_xattr(path) {
        return false;
    }

    metadata.blocks() == 0 && (metadata.len() > 0 || is_file_provider_path(path))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_is_cloud_only(_path: &Path, _metadata: &Metadata) -> bool {
    false
}

#[cfg(target_os = "macos")]
fn has_cloud_xattr(path: &Path) -> bool {
    let Ok(path) = CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };

    unsafe {
        let size = libc::listxattr(path.as_ptr(), std::ptr::null_mut(), 0, libc::XATTR_NOFOLLOW);
        if size <= 0 {
            return false;
        }

        let mut buf = vec![0u8; size as usize];
        let len = libc::listxattr(
            path.as_ptr(),
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
            libc::XATTR_NOFOLLOW,
        );
        if len <= 0 {
            return false;
        }

        buf[..len as usize]
            .split(|byte| *byte == 0)
            .filter_map(|name| std::str::from_utf8(name).ok())
            .any(|name| {
                CLOUD_XATTR_PREFIXES
                    .iter()
                    .any(|prefix| name.starts_with(prefix))
            })
    }
}

#[cfg(target_os = "macos")]
fn is_file_provider_path(path: &Path) -> bool {
    path.components().any(|component| {
        component
            .as_os_str()
            .to_str()
            .is_some_and(|part| part == "CloudStorage" || part.contains("Mobile Documents"))
    })
}
