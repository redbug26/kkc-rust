use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;
const MAX_CACHEABLE_ENTRY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct CachedRead {
    pub bytes: Vec<u8>,
    pub file_len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FileKey {
    path: PathBuf,
    len: u64,
    modified_ns: Option<u128>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TailKey {
    file: FileKey,
    len: usize,
}

#[derive(Debug, Clone)]
struct PrefixEntry {
    bytes: Vec<u8>,
    complete: bool,
}

#[derive(Default)]
struct FileCache {
    prefixes: HashMap<FileKey, PrefixEntry>,
    tails: HashMap<TailKey, Vec<u8>>,
}

fn cache() -> &'static Mutex<FileCache> {
    static CACHE: OnceLock<Mutex<FileCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(FileCache::default()))
}

pub fn read_file(path: &Path, max_bytes: Option<usize>) -> io::Result<CachedRead> {
    match max_bytes {
        Some(max_bytes) => read_prefix(path, max_bytes),
        None => read_full(path),
    }
}

pub fn read_prefix(path: &Path, max_bytes: usize) -> io::Result<CachedRead> {
    let meta = fs::metadata(path)?;
    let key = file_key(path, &meta);
    let file_len = meta.len();
    let wanted = (file_len as usize).min(max_bytes);

    if let Ok(guard) = cache().lock()
        && let Some(entry) = guard.prefixes.get(&key)
        && (entry.bytes.len() >= wanted || entry.complete)
    {
        let bytes = entry.bytes[..entry.bytes.len().min(wanted)].to_vec();
        return Ok(CachedRead { bytes, file_len });
    }

    let mut file = fs::File::open(path)?;
    let mut bytes = vec![0u8; wanted];
    let read = file.read(&mut bytes)?;
    bytes.truncate(read);
    let complete = bytes.len() as u64 == file_len;

    if bytes.len() <= MAX_CACHEABLE_ENTRY_BYTES
        && let Ok(mut guard) = cache().lock()
    {
        guard.prefixes.insert(
            key,
            PrefixEntry {
                bytes: bytes.clone(),
                complete,
            },
        );
        guard.trim();
    }

    Ok(CachedRead { bytes, file_len })
}

pub fn read_tail(path: &Path, tail_len: usize) -> io::Result<CachedRead> {
    let meta = fs::metadata(path)?;
    let file_len = meta.len();
    let key = file_key(path, &meta);
    let wanted = (file_len as usize).min(tail_len);
    let tail_key = TailKey {
        file: key.clone(),
        len: wanted,
    };

    if let Ok(guard) = cache().lock()
        && let Some(bytes) = guard.tails.get(&tail_key)
    {
        return Ok(CachedRead {
            bytes: bytes.clone(),
            file_len,
        });
    }

    let mut file = fs::File::open(path)?;
    if wanted > 0 {
        file.seek(SeekFrom::End(-(wanted as i64)))?;
    }
    let mut bytes = vec![0u8; wanted];
    if wanted > 0 {
        file.read_exact(&mut bytes)?;
    }

    if bytes.len() <= MAX_CACHEABLE_ENTRY_BYTES
        && let Ok(mut guard) = cache().lock()
    {
        guard.tails.insert(tail_key, bytes.clone());
        if wanted as u64 == file_len {
            guard.prefixes.insert(
                key,
                PrefixEntry {
                    bytes: bytes.clone(),
                    complete: true,
                },
            );
        }
        guard.trim();
    }

    Ok(CachedRead { bytes, file_len })
}

fn read_full(path: &Path) -> io::Result<CachedRead> {
    let meta = fs::metadata(path)?;
    let key = file_key(path, &meta);
    let file_len = meta.len();

    if file_len as usize <= MAX_CACHEABLE_ENTRY_BYTES
        && let Ok(guard) = cache().lock()
        && let Some(entry) = guard.prefixes.get(&key)
        && entry.complete
    {
        return Ok(CachedRead {
            bytes: entry.bytes.clone(),
            file_len,
        });
    }

    let bytes = fs::read(path)?;
    if bytes.len() <= MAX_CACHEABLE_ENTRY_BYTES
        && let Ok(mut guard) = cache().lock()
    {
        guard.prefixes.insert(
            key,
            PrefixEntry {
                bytes: bytes.clone(),
                complete: true,
            },
        );
        guard.trim();
    }

    Ok(CachedRead { bytes, file_len })
}

fn file_key(path: &Path, meta: &fs::Metadata) -> FileKey {
    let modified_ns = meta
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos());
    FileKey {
        path: path.to_path_buf(),
        len: meta.len(),
        modified_ns,
    }
}

impl FileCache {
    fn trim(&mut self) {
        while self.total_bytes() > MAX_CACHE_BYTES {
            if let Some(key) = self
                .tails
                .iter()
                .max_by_key(|(_, bytes)| bytes.len())
                .map(|(key, _)| key.clone())
            {
                self.tails.remove(&key);
                continue;
            }
            if let Some(key) = self
                .prefixes
                .iter()
                .max_by_key(|(_, entry)| entry.bytes.len())
                .map(|(key, _)| key.clone())
            {
                self.prefixes.remove(&key);
                continue;
            }
            break;
        }
    }

    fn total_bytes(&self) -> usize {
        self.prefixes
            .values()
            .map(|entry| entry.bytes.len())
            .sum::<usize>()
            + self.tails.values().map(Vec::len).sum::<usize>()
    }
}
