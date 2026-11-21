use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Default maximum number of cached PNG data URIs in the unified `IconCache` (AD-6).
pub const DEFAULT_ICON_CACHE_CAPACITY: usize = 256;

/// Default TTL in milliseconds for negative icon-extraction misses (prevents 650ms poller thrash).
pub const DEFAULT_NEGATIVE_MISS_TTL_MS: u64 = 30_000;

/// Maximum number of bounded negative-miss entries tracked simultaneously.
pub const MAX_NEGATIVE_MISS_ENTRIES: usize = 256;

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
    let mut norm = path.trim().replace('/', "\\").to_lowercase();
    while norm.len() > 3 && norm.ends_with('\\') {
        norm.pop();
    }
    norm
}

fn normalize_aumid(aumid: &str) -> String {
    let trimmed = aumid.trim();
    let lower = trimmed.to_lowercase();
    if let Some(rest) = lower.strip_prefix("shell:appsfolder\\") {
        rest.trim().to_string()
    } else {
        lower
    }
}

/// Builds a deterministic namespaced key for executable paths (`exe:<hash>`).
pub fn exe_key(path: &str) -> String {
    format!("exe:{:016x}", fnv1a_64(&normalize_path(path)))
}

/// Builds a deterministic namespaced key for packaged UWP AUMIDs (`aumid:<hash>`).
pub fn aumid_key(aumid: &str) -> String {
    format!("aumid:{:016x}", fnv1a_64(&normalize_aumid(aumid)))
}

/// Builds a namespaced key for live window HICON fallback icons (`hwnd:<hwnd>:<rev>`).
pub fn hwnd_key(hwnd: isize, rev: u32) -> String {
    format!("hwnd:{hwnd}:{rev}")
}

/// Builds a deterministic namespaced key for generic files/folders (`file:<hash>`).
pub fn file_key(path: &str) -> String {
    format!("file:{:016x}", fnv1a_64(&normalize_path(path)))
}

/// Chooses `exe:` for `.exe` paths and `file:` for all other file/folder paths.
pub fn path_icon_key(path: &str) -> String {
    let norm = normalize_path(path);
    if norm.ends_with(".exe") {
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

/// Bounded LRU icon cache mapping namespaced `icon_key` strings to PNG data URIs (AD-6),
/// plus a bounded TTL negative-miss cache so iconless/elevated apps do not thrash GDI/`WM_GETICON`.
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

    /// Records a failed icon extraction for `key` for up to `negative_ttl` (bounded to `MAX_NEGATIVE_MISS_ENTRIES`).
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
        self.put(key.to_string(), loaded.clone());
        Some(loaded)
    }

    /// Like `get_or_insert_with`, but suppresses repeated `loader` invocations while `key`
    /// is negatively cached within its TTL window.
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
                self.put(key.to_string(), loaded.clone());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_capacity_and_zero_clamping() {
        let def = IconCache::default();
        assert_eq!(def.capacity(), 256);
        assert!(def.is_empty());

        let min_cap = IconCache::new(0);
        assert_eq!(min_cap.capacity(), 1);
    }

    #[test]
    fn namespaced_key_builders_normalize_and_separate_namespaces() {
        let exe_a = exe_key(r"C:\Program Files\Zen\Zen.EXE");
        let exe_b = exe_key("  c:/program files/zen/zen.exe  ");
        assert_eq!(exe_a, exe_b);
        assert!(exe_a.starts_with("exe:"));
        assert_eq!(exe_a.len(), 4 + 16);

        let file_a = file_key(r"C:\Users\Test\Docs\");
        let file_b = file_key("c:/users/test/docs");
        assert_eq!(file_a, file_b);
        assert!(file_a.starts_with("file:"));

        // Distinct namespaces even for identical path input
        assert_ne!(
            exe_key(r"C:\Windows\notepad.exe"),
            file_key(r"C:\Windows\notepad.exe")
        );
        assert_eq!(
            path_icon_key(r"C:\Windows\Notepad.EXE"),
            exe_key(r"C:\Windows\notepad.exe")
        );
        assert_eq!(
            path_icon_key(r"C:\Users\Test\readme.txt"),
            file_key(r"C:\Users\Test\readme.txt")
        );

        let uwp_a = aumid_key("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App");
        let uwp_b = aumid_key("shell:AppsFolder\\microsoft.windowscalculator_8wekyb3d8bbwe!app");
        assert_eq!(uwp_a, uwp_b);
        assert!(uwp_a.starts_with("aumid:"));
        assert_eq!(uwp_a.len(), 6 + 16);

        assert_eq!(hwnd_key(0x1234, 2), "hwnd:4660:2");
        assert_ne!(hwnd_key(0x1234, 0), hwnd_key(0x1234, 1));
    }

    #[test]
    fn lru_eviction_updates_recency_on_get_put_and_batch() {
        let mut cache = IconCache::new(3);
        cache.put("exe:1".into(), "data:1".into());
        cache.put("exe:2".into(), "data:2".into());
        cache.put("exe:3".into(), "data:3".into());
        assert_eq!(cache.len(), 3);

        // Touch exe:1 so exe:2 becomes the LRU entry
        assert_eq!(cache.get("exe:1").as_deref(), Some("data:1"));

        // Inserting exe:4 must evict exe:2, keeping exe:1, exe:3, exe:4
        cache.put("exe:4".into(), "data:4".into());
        assert_eq!(cache.len(), 3);
        assert_eq!(cache.get("exe:2"), None);
        assert!(cache.contains_key("exe:1"));
        assert!(cache.contains_key("exe:3"));
        assert!(cache.contains_key("exe:4"));

        // Overwriting an existing key updates value and recency without evicting
        cache.put("exe:3".into(), "data:3-updated".into());
        assert_eq!(cache.len(), 3);
        assert_eq!(cache.get("exe:3").as_deref(), Some("data:3-updated"));

        // Touch exe:1 and exe:4 via get_batch; then exe:3 is touched earlier? Let's verify exact order:
        // Current recency after above: exe:4 < exe:1 < exe:3.
        // Batch touch exe:4 -> order becomes exe:1 < exe:3 < exe:4.
        let batch = cache.get_batch(&[
            "exe:4".into(),
            "exe:missing".into(),
            "exe:4".into(),
            "".into(),
        ]);
        assert_eq!(batch.len(), 1);
        assert_eq!(batch.get("exe:4").map(String::as_str), Some("data:4"));

        // Inserting exe:5 must now evict exe:1 (the LRU), keeping exe:3, exe:4, exe:5
        cache.put("exe:5".into(), "data:5".into());
        assert!(!cache.contains_key("exe:1"));
        assert!(cache.contains_key("exe:3"));
        assert!(cache.contains_key("exe:4"));
        assert!(cache.contains_key("exe:5"));
    }

    #[test]
    fn get_or_insert_with_caches_some_and_skips_none() {
        let mut cache = IconCache::new(4);
        let mut calls = 0usize;

        let none_res = cache.get_or_insert_with("exe:empty", || {
            calls += 1;
            None
        });
        assert_eq!(none_res, None);
        assert_eq!(calls, 1);
        assert!(cache.is_empty());

        let first = cache.get_or_insert_with("exe:ok", || {
            calls += 1;
            Some("data:image/png;base64,AAA".to_string())
        });
        assert_eq!(first.as_deref(), Some("data:image/png;base64,AAA"));
        assert_eq!(calls, 2);

        let second = cache.get_or_insert_with("exe:ok", || {
            calls += 1;
            Some("data:image/png;base64,BBB".to_string())
        });
        assert_eq!(second.as_deref(), Some("data:image/png;base64,AAA"));
        assert_eq!(calls, 2, "loader must not run on cache hit");
    }

    #[test]
    fn capacity_256_evicts_oldest_on_overflow() {
        let mut cache = IconCache::default();
        for i in 0..260 {
            cache.put(format!("exe:{i}"), format!("data:{i}"));
        }
        assert_eq!(cache.len(), 256);
        for evicted in 0..4 {
            assert!(!cache.contains_key(&format!("exe:{evicted}")));
        }
        for kept in 4..260 {
            assert!(cache.contains_key(&format!("exe:{kept}")));
        }
    }

    #[test]
    fn negative_miss_cache_suppresses_repeated_loads_expires_and_clears_on_put() {
        let mut cache = IconCache::with_negative_ttl(8, Duration::from_millis(60));
        let mut calls = 0usize;

        // First miss invokes loader and records negative miss
        assert_eq!(
            cache.get_or_insert_with_negative_cache("exe:missing", || {
                calls += 1;
                None
            }),
            None
        );
        assert_eq!(calls, 1);
        assert!(cache.is_negative_cached("exe:missing"));
        assert_eq!(cache.negative_miss_len(), 1);

        // Second call within TTL skips loader completely
        assert_eq!(
            cache.get_or_insert_with_negative_cache("exe:missing", || {
                calls += 1;
                Some("data:image/png;base64,SHOULD_NOT_RUN".into())
            }),
            None
        );
        assert_eq!(calls, 1);

        // Explicit put clears the negative miss immediately
        cache.put("exe:missing".into(), "data:image/png;base64,NOW_OK".into());
        assert!(!cache.is_negative_cached("exe:missing"));
        assert_eq!(
            cache.get("exe:missing").as_deref(),
            Some("data:image/png;base64,NOW_OK")
        );

        // TTL expiry allows retry after deadline
        cache.mark_negative_miss("exe:ttl".into());
        assert!(cache.is_negative_cached("exe:ttl"));
        std::thread::sleep(Duration::from_millis(75));
        assert!(!cache.is_negative_cached("exe:ttl"));

        // Negative miss table stays bounded by MAX_NEGATIVE_MISS_ENTRIES
        let mut bounded = IconCache::with_negative_ttl(8, Duration::from_secs(60));
        for i in 0..(MAX_NEGATIVE_MISS_ENTRIES + 20) {
            bounded.mark_negative_miss(format!("exe:miss-{i}"));
        }
        assert!(bounded.negative_miss_len() <= MAX_NEGATIVE_MISS_ENTRIES);
    }
}
