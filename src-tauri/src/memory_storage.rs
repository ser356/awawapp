//! In-memory torrent storage backend with LZ4 compression (zram-style).
//!
//! Pieces are stored in sparse, **compressed** RAM pages and never written to disk.
//! Each 1 MiB page is LZ4-compressed before being stored in the HashMap,
//! similar to how Linux zram compresses swap pages in memory.
//!
//! For incompressible data (e.g. already-compressed video) the overhead is
//! minimal (~1-2%). For compressible data the savings can reach 50-70%.

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

// ─── Compressed page ────────────────────────────────────────────────────────

/// A page stored in LZ4-compressed form.
struct CompressedPage {
    /// LZ4-compressed bytes of the full PAGE_SIZE page.
    data: Vec<u8>,
}

impl CompressedPage {
    /// Compress a raw page into LZ4.
    fn compress(raw: &[u8]) -> Self {
        Self {
            data: lz4_flex::compress_prepend_size(raw),
        }
    }

    /// Decompress back to the original PAGE_SIZE bytes.
    fn decompress(&self) -> anyhow::Result<Vec<u8>> {
        lz4_flex::decompress_size_prepended(&self.data)
            .map_err(|e| anyhow::anyhow!("LZ4 decompress failed: {e}"))
    }

    /// Bytes actually used in RAM (the compressed representation).
    #[allow(dead_code)]
    fn compressed_size(&self) -> usize {
        self.data.len()
    }
}

// ─── Storage ─────────────────────────────────────────────────────────────────

pub struct InMemoryStorage {
    /// Sparse page map: (file_id, page_index) → LZ4-compressed page.
    pages: Arc<RwLock<HashMap<(usize, u64), CompressedPage>>>,
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

            let compressed = pages
                .get(&(file_id, page_idx))
                .ok_or_else(|| anyhow::anyhow!("piece not downloaded yet (file={file_id}, page={page_idx})"))?;

            let raw = compressed.decompress()?;
            buf[buf_pos..buf_pos + chunk].copy_from_slice(&raw[page_off..page_off + chunk]);
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

            // Decompress existing page or create a fresh zeroed one.
            let mut raw = if let Some(existing) = pages.get(&(file_id, page_idx)) {
                existing.decompress()?
            } else {
                vec![0u8; PAGE_SIZE as usize]
            };

            raw[page_off..page_off + chunk].copy_from_slice(&buf[buf_pos..buf_pos + chunk]);

            // Re-compress and store.
            pages.insert((file_id, page_idx), CompressedPage::compress(&raw));

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
