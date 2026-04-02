//! In-memory torrent storage backend.
//!
//! Pieces are stored in sparse RAM pages and never written to disk.
//! This replicates Stremio-style ephemeral streaming: no storage usage,
//! all data is freed when the torrent is dropped.
//!
//! Implementation: a HashMap<(file_id, page_index), Vec<u8>> where each
//! page is PAGE_SIZE bytes. Pages are lazily allocated on first write,
//! so a 100 GB torrent only uses RAM proportional to what was downloaded.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

use librqbit::storage::{BoxStorageFactory, StorageFactory, StorageFactoryExt, TorrentStorage};
use librqbit::{ManagedTorrentShared, TorrentMetadata};

/// 1 MiB pages — matches the most common torrent piece size.
const PAGE_SIZE: u64 = 1024 * 1024;

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

// ─── Storage ─────────────────────────────────────────────────────────────────

pub struct InMemoryStorage {
    /// Sparse page map: (file_id, page_index) → page bytes.
    pages: Arc<RwLock<HashMap<(usize, u64), Vec<u8>>>>,
}

impl InMemoryStorage {
    fn new() -> Self {
        Self {
            pages: Arc::new(RwLock::new(HashMap::new())),
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
        let pages = self.pages.read().map_err(|_| anyhow::anyhow!("lock poisoned"))?;
        let mut remaining = buf.len();
        let mut buf_pos = 0usize;
        let mut cur = offset;

        while remaining > 0 {
            let page_idx = cur / PAGE_SIZE;
            let page_off = (cur % PAGE_SIZE) as usize;
            let chunk = (PAGE_SIZE as usize - page_off).min(remaining);

            let page = pages
                .get(&(file_id, page_idx))
                .ok_or_else(|| anyhow::anyhow!("piece not downloaded yet (file={file_id}, page={page_idx})"))?;

            buf[buf_pos..buf_pos + chunk].copy_from_slice(&page[page_off..page_off + chunk]);
            buf_pos += chunk;
            cur += chunk as u64;
            remaining -= chunk;
        }
        Ok(())
    }

    fn pwrite_all(&self, file_id: usize, offset: u64, buf: &[u8]) -> anyhow::Result<()> {
        let mut pages = self.pages.write().map_err(|_| anyhow::anyhow!("lock poisoned"))?;
        let mut remaining = buf.len();
        let mut buf_pos = 0usize;
        let mut cur = offset;

        while remaining > 0 {
            let page_idx = cur / PAGE_SIZE;
            let page_off = (cur % PAGE_SIZE) as usize;
            let chunk = (PAGE_SIZE as usize - page_off).min(remaining);

            let page = pages
                .entry((file_id, page_idx))
                .or_insert_with(|| vec![0u8; PAGE_SIZE as usize]);

            page[page_off..page_off + chunk].copy_from_slice(&buf[buf_pos..buf_pos + chunk]);
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
        let pages = {
            let mut g = self
                .pages
                .write()
                .map_err(|_| anyhow::anyhow!("lock poisoned"))?;
            std::mem::take(&mut *g)
        };
        Ok(Box::new(InMemoryStorage {
            pages: Arc::new(RwLock::new(pages)),
        }))
    }
}
