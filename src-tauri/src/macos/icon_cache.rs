use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::macos::bridge_ffi;

/// Default maximum number of cached PNG data URIs in the macOS `IconCache`.
pub const DEFAULT_ICON_CACHE_CAPACITY: usize = 384;

/// Default TTL in milliseconds for negative icon-extraction misses.
pub const DEFAULT_NEGATIVE_MISS_TTL_MS: u64 = 30_000;

/// Maximum number of bounded negative-miss entries tracked simultaneously.
pub const MAX_NEGATIVE_MISS_ENTRIES: usize = 384;

const FNV1A_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV1A_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv1a_64(input: &str) -> u64 {
    let mut hash = FNV1A_OFFSET_BASIS;
    for byte in input.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV1A_PRIME);
    }
    hash
}

fn normalize_path(path: &str) -> String {
    let trimmed = path.trim().replace('\\', "/");
    let mut collapsed = String::with_capacity(trimmed.len());
    let mut prev_slash = false;
    for ch in trimmed.chars() {
        if ch == '/' {
            if !prev_slash {
                collapsed.push('/');
                prev_slash = true;
            }
        } else {
            collapsed.push(ch);
            prev_slash = false;
        }
    }
    while collapsed.len() > 1 && collapsed.ends_with('/') {
        collapsed.pop();
    }
    collapsed.to_lowercase()
}

/// Extracts the outermost `.app` bundle path from an executable inside a macOS bundle.
pub fn app_bundle_ancestor(path: &str) -> Option<String> {
    let trimmed = path.trim().replace('\\', "/");
    let low = trimmed.to_ascii_lowercase();
    if let Some(idx) = low.find(".app/") {
        return Some(trimmed[..idx + 4].to_string());
    }
    if low.ends_with(".app") {
        let mut s = trimmed;
        while s.len() > 1 && s.ends_with('/') {
            s.pop();
        }
        return Some(s);
    }
    None
}

/// Builds a deterministic namespaced key for macOS `.app` bundles / binaries (`exe:<16hex>`).
pub fn exe_key(path: &str) -> String {
    format!("exe:{:016x}", fnv1a_64(&normalize_path(path)))
}

/// Builds a deterministic namespaced key for generic files/folders (`file:<16hex>`).
pub fn file_key(path: &str) -> String {
    format!("file:{:016x}", fnv1a_64(&normalize_path(path)))
}

/// Chooses `exe:` for `.app` bundles and executables, `file:` otherwise.
pub fn path_icon_key(path: &str) -> String {
    let norm = normalize_path(path);
    let is_exe_like = norm.ends_with(".app")
        || norm.contains(".app/contents/macos/")
        || norm.ends_with(".command")
        || norm.ends_with(".sh")
        || norm.starts_with("/bin/")
        || norm.starts_with("/sbin/")
        || norm.starts_with("/usr/bin/")
        || norm.starts_with("/usr/local/bin/")
        || norm.starts_with("/opt/homebrew/bin/");
    if is_exe_like {
        format!("exe:{:016x}", fnv1a_64(&norm))
    } else {
        format!("file:{:016x}", fnv1a_64(&norm))
    }
}

#[derive(Debug, Clone)]
struct CacheEntry {
    value: String,
    last_used: u64,
}

/// Bounded LRU icon cache mapping namespaced `icon_key` strings to PNG data URIs,
/// plus a bounded TTL negative-miss cache so missing/iconless targets do not thrash `NSWorkspace`.
#[derive(Debug)]
pub struct IconCache {
    capacity: usize,
    tick: u64,
    entries: HashMap<String, CacheEntry>,
    negative_ttl: Duration,
    negative_misses: HashMap<String, Instant>,
}

impl Default for IconCache {
    fn default() -> Self {
        Self::new(DEFAULT_ICON_CACHE_CAPACITY)
    }
}

impl IconCache {
    pub fn new(capacity: usize) -> Self {
        Self::with_negative_ttl(capacity, Duration::from_millis(DEFAULT_NEGATIVE_MISS_TTL_MS))
    }

    pub fn with_negative_ttl(capacity: usize, negative_ttl: Duration) -> Self {
        let cap = capacity.max(1);
        Self {
            capacity: cap,
            tick: 0,
            entries: HashMap::with_capacity(cap.min(DEFAULT_ICON_CACHE_CAPACITY)),
            negative_ttl,
            negative_misses: HashMap::new(),
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    pub fn negative_miss_len(&self) -> usize {
        self.negative_misses.len()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.negative_misses.clear();
        self.tick = 0;
    }

    /// Returns `true` if `key` recently failed extraction and its negative-cache TTL has not expired.
    pub fn is_negative_cached(&mut self, key: &str) -> bool {
        if self.negative_ttl.is_zero() {
            return false;
        }
        let Some(&recorded_at) = self.negative_misses.get(key) else {
            return false;
        };
        if recorded_at.elapsed() < self.negative_ttl {
            true
        } else {
            self.negative_misses.remove(key);
            false
        }
    }

    /// Records a failed icon extraction for `key` for up to `negative_ttl`.
    pub fn mark_negative_miss(&mut self, key: String) {
        if key.is_empty() || self.negative_ttl.is_zero() {
            return;
        }
        let now = Instant::now();
        if self.negative_misses.len() >= MAX_NEGATIVE_MISS_ENTRIES
            && !self.negative_misses.contains_key(&key)
        {
            let ttl = self.negative_ttl;
            self.negative_misses
                .retain(|_, ts| now.saturating_duration_since(*ts) < ttl);
            if self.negative_misses.len() >= MAX_NEGATIVE_MISS_ENTRIES {
                if let Some(oldest) = self
                    .negative_misses
                    .iter()
                    .min_by_key(|(_, ts)| **ts)
                    .map(|(k, _)| k.clone())
                {
                    self.negative_misses.remove(&oldest);
                }
            }
        }
        self.negative_misses.insert(key, now);
    }

    pub fn clear_negative_miss(&mut self, key: &str) {
        self.negative_misses.remove(key);
    }

    fn next_tick(&mut self) -> u64 {
        self.tick = self.tick.wrapping_add(1);
        self.tick
    }

    /// Returns a cached PNG data URI and refreshes its LRU recency.
    pub fn get(&mut self, key: &str) -> Option<String> {
        let now = self.next_tick();
        if let Some(entry) = self.entries.get_mut(key) {
            entry.last_used = now;
            return Some(entry.value.clone());
        }
        None
    }

    /// Inserts or updates an icon entry, evicting the least-recently-used entry if full.
    pub fn insert(&mut self, key: String, data_uri: String) {
        self.put(key, data_uri);
    }

    /// Inserts or updates an icon entry, evicting the least-recently-used entry if full.
    pub fn put(&mut self, key: String, data_uri: String) {
        self.negative_misses.remove(&key);
        let now = self.next_tick();
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.value = data_uri;
            entry.last_used = now;
            return;
        }

        while self.entries.len() >= self.capacity {
            let lru_key = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(k, _)| k.clone());
            if let Some(evict) = lru_key {
                self.entries.remove(&evict);
            } else {
                break;
            }
        }

        self.entries.insert(
            key,
            CacheEntry {
                value: data_uri,
                last_used: now,
            },
        );
    }

    /// Returns the cached entry if present, otherwise invokes `loader` and caches `Some(uri)`.
    pub fn get_or_insert_with<F>(&mut self, key: &str, loader: F) -> Option<String>
    where
        F: FnOnce() -> Option<String>,
    {
        if let Some(hit) = self.get(key) {
            return Some(hit);
        }
        let loaded = loader()?;
        self.insert(key.to_string(), loaded.clone());
        Some(loaded)
    }

    /// Like `get_or_insert_with`, suppressing repeated `loader` calls while negatively cached.
    pub fn get_or_insert_with_negative_cache<F>(&mut self, key: &str, loader: F) -> Option<String>
    where
        F: FnOnce() -> Option<String>,
    {
        if let Some(hit) = self.get(key) {
            return Some(hit);
        }
        if self.is_negative_cached(key) {
            return None;
        }
        match loader() {
            Some(loaded) => {
                self.insert(key.to_string(), loaded.clone());
                Some(loaded)
            }
            None => {
                self.mark_negative_miss(key.to_string());
                None
            }
        }
    }

    /// Batch-fetches all cached icons matching `keys`, refreshing LRU recency for every hit.
    pub fn get_batch(&mut self, keys: &[String]) -> HashMap<String, String> {
        let mut out = HashMap::with_capacity(keys.len());
        for key in keys {
            if key.is_empty() || out.contains_key(key) {
                continue;
            }
            if let Some(uri) = self.get(key) {
                out.insert(key.clone(), uri);
            }
        }
        out
    }
}

/// Resolves an icon PNG data URI for `path` using `cache`, dropping the lock before FFI.
pub fn extract_icon_for_path_cached(path: &str, cache: &Mutex<IconCache>) -> Option<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let key = path_icon_key(trimmed);
    {
        let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(hit) = guard.get(&key) {
            return Some(hit);
        }
        if guard.is_negative_cached(&key) {
            return None;
        }
    }

    let extracted = bridge_ffi::icon_png_for_path(trimmed, 64);
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    match extracted {
        Some(uri) => {
            guard.insert(key, uri.clone());
            Some(uri)
        }
        None => {
            guard.mark_negative_miss(key);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_capacity_is_384() {
        let cache = IconCache::default();
        assert_eq!(cache.capacity(), 384);
        assert!(cache.is_empty());
        assert_eq!(IconCache::new(0).capacity(), 1);
    }

    #[test]
    fn key_builders_normalize_posix_paths() {
        assert_eq!(
            exe_key("/Applications/Safari.app/"),
            exe_key("  //applications//safari.app ")
        );
        assert_eq!(
            path_icon_key("/Applications/Safari.app"),
            exe_key("/Applications/Safari.app")
        );
        assert_eq!(
            path_icon_key("/Users/alex/Notes.txt"),
            file_key("/Users/alex/Notes.txt")
        );
        assert_ne!(
            exe_key("/Applications/Safari.app"),
            file_key("/Applications/Safari.app")
        );
    }

    #[test]
    fn extracts_outermost_app_bundle_ancestor() {
        assert_eq!(
            app_bundle_ancestor(
                "/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Helpers/Google Chrome Helper (Renderer).app/Contents/MacOS/Google Chrome Helper (Renderer)"
            ).as_deref(),
            Some("/Applications/Google Chrome.app")
        );
        assert_eq!(
            app_bundle_ancestor("/Applications/Safari.app").as_deref(),
            Some("/Applications/Safari.app")
        );
        assert_eq!(app_bundle_ancestor("/usr/bin/git"), None);
    }

    #[test]
    fn lru_eviction_and_negative_cache_behave_correctly() {
        let mut cache = IconCache::with_negative_ttl(2, Duration::from_millis(40));
        cache.insert("exe:a".into(), "data:a".into());
        cache.insert("exe:b".into(), "data:b".into());
        assert_eq!(cache.get("exe:a").as_deref(), Some("data:a"));
        cache.insert("exe:c".into(), "data:c".into());
        assert_eq!(cache.get("exe:b"), None);
        assert!(cache.contains_key("exe:a"));
        assert!(cache.contains_key("exe:c"));

        let mut calls = 0usize;
        assert_eq!(
            cache.get_or_insert_with_negative_cache("exe:miss", || {
                calls += 1;
                None
            }),
            None
        );
        assert_eq!(
            cache.get_or_insert_with_negative_cache("exe:miss", || {
                calls += 1;
                Some("should-not-run".into())
            }),
            None
        );
        assert_eq!(calls, 1);
        std::thread::sleep(Duration::from_millis(55));
        assert!(!cache.is_negative_cached("exe:miss"));
    }
}
