//! In-memory torrent storage with LRU eviction for streaming.
//!
//! Uses a circular buffer that keeps only the most recently accessed pages
//! in memory. Old pages are automatically evicted when the buffer is full.
//! This allows streaming torrents of any size without filling RAM.
//!
//! Default: 512 MiB buffer (~500 pages of 1 MiB each).

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use librqbit::storage::{BoxStorageFactory, StorageFactory, StorageFactoryExt, TorrentStorage};
use librqbit::{ManagedTorrentShared, TorrentMetadata};
use linked_hash_map::LinkedHashMap;

/// 1 MiB pages — matches the most common torrent piece size.
const PAGE_SIZE: u64 = 1024 * 1024;

/// Maximum buffer size in bytes (1 GiB for high-bitrate content like BD Remux).
/// BD Remux can have 30-50 Mbps bitrate, so we need more buffer.
const MAX_BUFFER_BYTES: usize = 1024 * 1024 * 1024;

/// Maximum number of pages to keep in memory.
const MAX_PAGES: usize = MAX_BUFFER_BYTES / PAGE_SIZE as usize;

/// How long to wait for a missing piece before giving up.
/// Longer timeout for initial pieces which may take time to download.
const PIECE_WAIT_TIMEOUT: Duration = Duration::from_secs(120);

/// Info hash type (20 bytes)
type InfoHash = [u8; 20];

// ─── Factory ─────────────────────────────────────────────────────────────────

/// Factory that creates `InMemoryStorage` instances and keeps a handle to each
/// storage's `wait_for_pieces` flag so it can be flipped on from the outside
/// when streaming begins.
#[derive(Clone)]
pub struct InMemoryStorageFactory {
    /// Shared map of wait-flags, keyed by torrent info_hash.
    /// Using info_hash ensures correct mapping regardless of librqbit's internal indices.
    wait_flags: Arc<Mutex<HashMap<InfoHash, Arc<AtomicBool>>>>,
}

impl Default for InMemoryStorageFactory {
    fn default() -> Self {
        Self {
            wait_flags: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl InMemoryStorageFactory {
    #[allow(dead_code)]
    pub fn boxed_factory() -> BoxStorageFactory {
        Self::default().boxed()
    }

    /// Enable wait-for-pieces mode on the storage for the given info_hash.
    pub fn enable_wait(&self, info_hash: &[u8; 20]) {
        if let Ok(flags) = self.wait_flags.lock() {
            tracing::info!("enable_wait called for {:?}, have {} storages", hex::encode(info_hash), flags.len());
            for (k, _) in flags.iter() {
                tracing::info!("  - stored hash: {:?}", hex::encode(k));
            }
            if let Some(flag) = flags.get(info_hash) {
                flag.store(true, Ordering::Release);
                tracing::info!("✓ Enabled wait-for-pieces for torrent {:?}", hex::encode(info_hash));
            } else {
                tracing::error!("✗ No storage found for info_hash {:?}", hex::encode(info_hash));
            }
        }
    }
}

impl StorageFactory for InMemoryStorageFactory {
    type Storage = InMemoryStorage;

    fn create(
        &self,
        shared: &ManagedTorrentShared,
        _metadata: &TorrentMetadata,
    ) -> anyhow::Result<Self::Storage> {
        let storage = InMemoryStorage::new();
        // Keep a handle to this storage's wait flag, keyed by info_hash.
        let info_hash = shared.info_hash.0;
        if let Ok(mut flags) = self.wait_flags.lock() {
            flags.insert(info_hash, storage.wait_for_pieces.clone());
            tracing::info!("Created storage for torrent {:?}", hex::encode(&info_hash));
        }
        Ok(storage)
    }

    fn clone_box(&self) -> BoxStorageFactory {
        self.clone().boxed()
    }
}

// ─── LRU Page Cache ─────────────────────────────────────────────────────────

type PageKey = (usize, u64); // (file_id, page_index)

/// LRU cache using LinkedHashMap for O(1) get/insert/evict operations.
/// The linked_hash_map crate maintains insertion order and allows
/// efficient re-ordering on access.
struct LruCache {
    /// Page data storage with LRU ordering built-in.
    pages: LinkedHashMap<PageKey, Vec<u8>>,
    /// Maximum number of pages to keep.
    max_pages: usize,
}

impl LruCache {
    fn new(max_pages: usize) -> Self {
        Self {
            pages: LinkedHashMap::with_capacity(max_pages),
            max_pages,
        }
    }

    /// Get a page, marking it as recently used. O(1) operation.
    fn get(&mut self, key: &PageKey) -> Option<&Vec<u8>> {
        // get_refresh moves the entry to the back (most recently used)
        // Convert &mut to & since we only need read access here
        self.pages.get_refresh(key).map(|v| &*v)
    }

    /// Get mutable access to a page for in-place modification. O(1).
    fn get_mut(&mut self, key: &PageKey) -> Option<&mut Vec<u8>> {
        self.pages.get_refresh(key)?;
        self.pages.get_mut(key)
    }

    /// Insert a page, evicting oldest pages if necessary. O(1) amortized.
    fn insert(&mut self, key: PageKey, data: Vec<u8>) {
        // If key already exists, remove it first to update position
        if self.pages.contains_key(&key) {
            self.pages.remove(&key);
        }

        // Evict oldest page if at capacity
        while self.pages.len() >= self.max_pages {
            // pop_front removes the oldest (least recently used) entry
            if self.pages.pop_front().is_none() {
                break;
            }
        }

        // Insert new page at the back (most recently used)
        self.pages.insert(key, data);
    }

    /// Check if a page exists (without updating LRU order). O(1).
    fn contains(&self, key: &PageKey) -> bool {
        self.pages.contains_key(key)
    }

    /// Get current memory usage in bytes.
    #[allow(dead_code)]
    fn memory_usage(&self) -> usize {
        self.pages.values().map(|v| v.len()).sum()
    }

    /// Get number of cached pages.
    #[allow(dead_code)]
    fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Clear all pages and release memory.
    #[allow(dead_code)]
    fn clear(&mut self) {
        self.pages.clear();
        self.pages.shrink_to_fit();
    }
}

// ─── Storage ─────────────────────────────────────────────────────────────────

pub struct InMemoryStorage {
    /// LRU page cache with automatic eviction.
    cache: Arc<Mutex<LruCache>>,
    /// Signalled every time a new page is written, so readers blocked on a
    /// missing piece can retry.
    page_written: Arc<Condvar>,
    /// When `false` (default), pread_exact fails immediately on missing pages.
    /// librqbit's initial checksum validation reads every piece — it must fail
    /// fast so the torrent can finish initializing.
    /// Set to `true` once streaming starts, so the HTTP stream handler waits
    /// for pieces that haven't been downloaded yet.
    wait_for_pieces: Arc<AtomicBool>,
}

impl InMemoryStorage {
    fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(LruCache::new(MAX_PAGES))),
            page_written: Arc::new(Condvar::new()),
            wait_for_pieces: Arc::new(AtomicBool::new(false)),
        }
    }

}

impl TorrentStorage for InMemoryStorage {
    fn init(
        &mut self,
        _shared: &ManagedTorrentShared,
        _metadata: &TorrentMetadata,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn pread_exact(&self, file_id: usize, offset: u64, buf: &mut [u8]) -> anyhow::Result<()> {
        let should_wait = self.wait_for_pieces.load(Ordering::Acquire);
        let mut remaining = buf.len();
        let mut buf_pos = 0usize;
        let mut cur = offset;

        // Log first read to help debug streaming issues
        static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !LOGGED.swap(true, Ordering::Relaxed) {
            tracing::info!("pread_exact called: file={}, offset={}, len={}, wait_mode={}", 
                file_id, offset, buf.len(), should_wait);
        }

        while remaining > 0 {
            let page_idx = cur / PAGE_SIZE;
            let page_off = (cur % PAGE_SIZE) as usize;
            let chunk = (PAGE_SIZE as usize - page_off).min(remaining);
            let key = (file_id, page_idx);

            let mut cache = self.cache.lock().map_err(|_| anyhow::anyhow!("lock poisoned"))?;

            if should_wait {
                // Streaming mode: wait for the page to be downloaded.
                let deadline = Instant::now() + PIECE_WAIT_TIMEOUT;
                loop {
                    if let Some(page) = cache.get(&key) {
                        buf[buf_pos..buf_pos + chunk]
                            .copy_from_slice(&page[page_off..page_off + chunk]);
                        break;
                    }

                    let now = Instant::now();
                    if now >= deadline {
                        return Err(anyhow::anyhow!(
                            "Timed out waiting for piece (file={file_id}, page={page_idx}). \
                             The torrent may have stalled or has no peers."
                        ));
                    }

                    let (guard, wait_result) = self
                        .page_written
                        .wait_timeout(cache, deadline - now)
                        .map_err(|_| anyhow::anyhow!("lock poisoned"))?;
                    cache = guard;

                    if wait_result.timed_out() && !cache.contains(&key) {
                        return Err(anyhow::anyhow!(
                            "Timed out waiting for piece (file={file_id}, page={page_idx}). \
                             The torrent may have stalled or has no peers."
                        ));
                    }
                }
            } else {
                // Fast-fail mode: used during librqbit's initial checksum
                // validation — must not block or init will never complete.
                let page = cache.get(&key).ok_or_else(|| {
                    anyhow::anyhow!("piece not downloaded yet (file={file_id}, page={page_idx})")
                })?;
                buf[buf_pos..buf_pos + chunk]
                    .copy_from_slice(&page[page_off..page_off + chunk]);
            }

            buf_pos += chunk;
            cur += chunk as u64;
            remaining -= chunk;
        }
        Ok(())
    }

    fn pwrite_all(&self, file_id: usize, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        {
            let mut cache = self.cache.lock().map_err(|_| anyhow::anyhow!("lock poisoned"))?;
            let mut remaining = buf.len();
            let mut buf_pos = 0usize;
            let mut cur = offset;

            while remaining > 0 {
                let page_idx = cur / PAGE_SIZE;
                let page_off = (cur % PAGE_SIZE) as usize;
                let chunk = (PAGE_SIZE as usize - page_off).min(remaining);

                let key = (file_id, page_idx);

                // Try to modify existing page in-place to avoid cloning
                if let Some(page) = cache.get_mut(&key) {
                    page[page_off..page_off + chunk]
                        .copy_from_slice(&buf[buf_pos..buf_pos + chunk]);
                } else {
                    // Create new page only when it doesn't exist
                    let mut raw = vec![0u8; PAGE_SIZE as usize];
                    raw[page_off..page_off + chunk]
                        .copy_from_slice(&buf[buf_pos..buf_pos + chunk]);
                    cache.insert(key, raw);
                }

                buf_pos += chunk;
                cur += chunk as u64;
                remaining -= chunk;
            }
        } // drop lock before notify

        // Wake up any readers waiting for pieces.
        self.page_written.notify_all();
        Ok(())
    }

    fn remove_file(&self, _file_id: usize, _filename: &Path) -> anyhow::Result<()> {
        Ok(()) // nothing on disk to remove
    }

    fn remove_directory_if_empty(&self, _path: &Path) -> anyhow::Result<()> {
        Ok(())
    }

    fn ensure_file_length(&self, _file_id: usize, _length: u64) -> anyhow::Result<()> {
        Ok(()) // sparse — no pre-allocation
    }

    fn take(&self) -> anyhow::Result<Box<dyn TorrentStorage>> {
        let cache = {
            let mut g = self
                .cache
                .lock()
                .map_err(|_| anyhow::anyhow!("lock poisoned"))?;
            std::mem::replace(&mut *g, LruCache::new(MAX_PAGES))
        };
        Ok(Box::new(InMemoryStorage {
            cache: Arc::new(Mutex::new(cache)),
            page_written: Arc::new(Condvar::new()),
            wait_for_pieces: Arc::new(AtomicBool::new(
                self.wait_for_pieces.load(Ordering::Relaxed),
            )),
        }))
    }
}
