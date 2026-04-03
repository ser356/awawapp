//! In-memory torrent storage with LRU eviction for streaming.
//!
//! Uses a circular buffer that keeps only the most recently accessed pages
//! in memory. Old pages are automatically evicted when the buffer is full.
//! This allows streaming torrents of any size without filling RAM.
//!
//! Default: 512 MiB buffer (~500 pages of 1 MiB each).

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex};

use librqbit::storage::{BoxStorageFactory, StorageFactory, StorageFactoryExt, TorrentStorage};
use librqbit::{ManagedTorrentShared, TorrentMetadata};

/// 1 MiB pages — matches the most common torrent piece size.
const PAGE_SIZE: u64 = 1024 * 1024;

/// Maximum buffer size in bytes (1 GiB default).
/// Adjust based on available RAM. For 4K streaming, 512MB-1GB is reasonable.
const MAX_BUFFER_BYTES: usize = 1024 * 1024 * 1024;

/// Maximum number of pages to keep in memory.
const MAX_PAGES: usize = MAX_BUFFER_BYTES / PAGE_SIZE as usize;

// ─── Factory ─────────────────────────────────────────────────────────────────

#[derive(Clone, Default)]
pub struct InMemoryStorageFactory;

impl InMemoryStorageFactory {
    #[allow(dead_code)]
    pub fn boxed_factory() -> BoxStorageFactory {
        Self.boxed()
    }
}

impl StorageFactory for InMemoryStorageFactory {
    type Storage = InMemoryStorage;

    fn create(
        &self,
        _shared: &ManagedTorrentShared,
        _metadata: &TorrentMetadata,
    ) -> anyhow::Result<Self::Storage> {
        Ok(InMemoryStorage::new())
    }

    fn clone_box(&self) -> BoxStorageFactory {
        self.clone().boxed()
    }
}

// ─── LRU Page Cache ─────────────────────────────────────────────────────────

type PageKey = (usize, u64); // (file_id, page_index)

struct LruCache {
    /// Page data storage.
    pages: HashMap<PageKey, Vec<u8>>,
    /// LRU order: front = oldest, back = newest.
    order: VecDeque<PageKey>,
    /// Maximum number of pages to keep.
    max_pages: usize,
}

impl LruCache {
    fn new(max_pages: usize) -> Self {
        Self {
            pages: HashMap::with_capacity(max_pages),
            order: VecDeque::with_capacity(max_pages),
            max_pages,
        }
    }

    /// Get a page, marking it as recently used.
    fn get(&mut self, key: &PageKey) -> Option<&Vec<u8>> {
        if self.pages.contains_key(key) {
            // Move to back (most recently used)
            self.order.retain(|k| k != key);
            self.order.push_back(*key);
            self.pages.get(key)
        } else {
            None
        }
    }

    /// Insert a page, evicting old pages if necessary.
    fn insert(&mut self, key: PageKey, data: Vec<u8>) {
        // If key already exists, update it
        if self.pages.contains_key(&key) {
            self.pages.insert(key, data);
            self.order.retain(|k| *k != key);
            self.order.push_back(key);
            return;
        }

        // Evict oldest pages if at capacity
        while self.pages.len() >= self.max_pages {
            if let Some(oldest) = self.order.pop_front() {
                self.pages.remove(&oldest);
            } else {
                break;
            }
        }

        // Insert new page
        self.pages.insert(key, data);
        self.order.push_back(key);
    }

    /// Check if a page exists (without updating LRU order).
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
}

// ─── Storage ─────────────────────────────────────────────────────────────────

pub struct InMemoryStorage {
    /// LRU page cache with automatic eviction.
    cache: Arc<Mutex<LruCache>>,
}

impl InMemoryStorage {
    fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(LruCache::new(MAX_PAGES))),
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
        let mut cache = self.cache.lock().map_err(|_| anyhow::anyhow!("lock poisoned"))?;
        let mut remaining = buf.len();
        let mut buf_pos = 0usize;
        let mut cur = offset;

        while remaining > 0 {
            let page_idx = cur / PAGE_SIZE;
            let page_off = (cur % PAGE_SIZE) as usize;
            let chunk = (PAGE_SIZE as usize - page_off).min(remaining);

            let key = (file_id, page_idx);
            let page = cache
                .get(&key)
                .ok_or_else(|| anyhow::anyhow!("piece not downloaded yet (file={file_id}, page={page_idx})"))?;

            buf[buf_pos..buf_pos + chunk].copy_from_slice(&page[page_off..page_off + chunk]);
            buf_pos += chunk;
            cur += chunk as u64;
            remaining -= chunk;
        }
        Ok(())
    }

    fn pwrite_all(&self, file_id: usize, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        let mut cache = self.cache.lock().map_err(|_| anyhow::anyhow!("lock poisoned"))?;
        let mut remaining = buf.len();
        let mut buf_pos = 0usize;
        let mut cur = offset;

        while remaining > 0 {
            let page_idx = cur / PAGE_SIZE;
            let page_off = (cur % PAGE_SIZE) as usize;
            let chunk = (PAGE_SIZE as usize - page_off).min(remaining);

            let key = (file_id, page_idx);

            // Get existing page or create a new zeroed one.
            let mut raw = if cache.contains(&key) {
                cache.get(&key).unwrap().clone()
            } else {
                vec![0u8; PAGE_SIZE as usize]
            };

            raw[page_off..page_off + chunk].copy_from_slice(&buf[buf_pos..buf_pos + chunk]);

            // Store page (old pages auto-evicted if at capacity).
            cache.insert(key, raw);

            buf_pos += chunk;
            cur += chunk as u64;
            remaining -= chunk;
        }
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
        }))
    }
}
