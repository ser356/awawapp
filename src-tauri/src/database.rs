//! SQLite database module for persistent magnet link history.
//! 
//! Security considerations:
//! - Uses parameterized queries to prevent SQL injection
//! - WAL mode for crash recovery and concurrent access
//! - Input validation before database operations

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

/// Represents a torrent entry in the history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TorrentHistory {
    pub id: i64,
    pub magnet_link: String,
    pub name: String,
    pub added_at: String,
    pub total_size: i64,
    pub status: String,
}

/// Thread-safe database wrapper
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// Initialize database with WAL mode for better concurrency
    pub fn new(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)
            .context("Failed to open database")?;
        
        // Enable WAL mode for crash recovery and concurrent access
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        
        // Create tables if they don't exist
        conn.execute(
            "CREATE TABLE IF NOT EXISTS torrent_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                magnet_link TEXT NOT NULL UNIQUE,
                name TEXT NOT NULL,
                added_at TEXT NOT NULL DEFAULT (datetime('now')),
                total_size INTEGER NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'added'
            )",
            [],
        )?;
        
        // Index for faster lookups
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_added_at ON torrent_history(added_at DESC)",
            [],
        )?;
        
        tracing::info!("Database initialized at {:?}", db_path);
        
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
    
    /// Add a new magnet link to history
    /// Returns the ID of the inserted/existing entry
    pub fn add_magnet(&self, magnet_link: &str, name: &str) -> Result<i64> {
        // Validate magnet link format (basic security check)
        if !magnet_link.starts_with("magnet:?") {
            anyhow::bail!("Invalid magnet link format");
        }
        self.add_torrent_entry(magnet_link, name)
    }

    /// Add a torrent file entry to history (no magnet validation)
    /// Returns the ID of the inserted/existing entry
    pub fn add_torrent_entry(&self, uri: &str, name: &str) -> Result<i64> {
        // Sanitize name - remove potentially dangerous characters
        let sanitized_name = sanitize_string(name);
        
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        
        // Use INSERT OR IGNORE to handle duplicates gracefully
        conn.execute(
            "INSERT OR IGNORE INTO torrent_history (magnet_link, name) VALUES (?1, ?2)",
            params![uri, sanitized_name],
        )?;
        
        // Get the ID (either newly inserted or existing)
        let id: i64 = conn.query_row(
            "SELECT id FROM torrent_history WHERE magnet_link = ?1",
            params![uri],
            |row| row.get(0),
        )?;
        
        tracing::info!("Added torrent to history: id={}", id);
        Ok(id)
    }
    
    /// Update torrent metadata after it's been fetched
    pub fn update_torrent_info(&self, id: i64, name: &str, total_size: i64) -> Result<()> {
        let sanitized_name = sanitize_string(name);
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        
        conn.execute(
            "UPDATE torrent_history SET name = ?1, total_size = ?2 WHERE id = ?3",
            params![sanitized_name, total_size, id],
        )?;
        
        Ok(())
    }
    
    /// Update torrent status
    pub fn update_status(&self, id: i64, status: &str) -> Result<()> {
        // Validate status to prevent arbitrary values
        let valid_statuses = ["added", "downloading", "seeding", "paused", "completed", "error"];
        if !valid_statuses.contains(&status) {
            anyhow::bail!("Invalid status value");
        }
        
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        
        conn.execute(
            "UPDATE torrent_history SET status = ?1 WHERE id = ?2",
            params![status, id],
        )?;
        
        Ok(())
    }
    
    /// Get all torrents from history, ordered by most recent first
    pub fn get_history(&self, limit: Option<u32>) -> Result<Vec<TorrentHistory>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        
        let limit = limit.unwrap_or(100).min(1000); // Cap at 1000 for safety
        
        let mut stmt = conn.prepare(
            "SELECT id, magnet_link, name, added_at, total_size, status 
             FROM torrent_history 
             ORDER BY added_at DESC 
             LIMIT ?1"
        )?;
        
        let rows = stmt.query_map(params![limit], |row| {
            Ok(TorrentHistory {
                id: row.get(0)?,
                magnet_link: row.get(1)?,
                name: row.get(2)?,
                added_at: row.get(3)?,
                total_size: row.get(4)?,
                status: row.get(5)?,
            })
        })?;
        
        let mut history = Vec::new();
        for row in rows {
            history.push(row?);
        }
        
        Ok(history)
    }
    
    /// Delete a torrent from history
    pub fn delete_torrent(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        
        conn.execute(
            "DELETE FROM torrent_history WHERE id = ?1",
            params![id],
        )?;
        
        tracing::info!("Deleted torrent from history: id={}", id);
        Ok(())
    }
    
    /// Search history by name
    pub fn search(&self, query: &str) -> Result<Vec<TorrentHistory>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        
        // Escape LIKE wildcards and limit query length
        let sanitized_query = query
            .chars()
            .take(100) // Limit query length
            .collect::<String>()
            .replace('%', "\\%")
            .replace('_', "\\_");
        
        let pattern = format!("%{}%", sanitized_query);
        
        let mut stmt = conn.prepare(
            "SELECT id, magnet_link, name, added_at, total_size, status 
             FROM torrent_history 
             WHERE name LIKE ?1 ESCAPE '\\'
             ORDER BY added_at DESC 
             LIMIT 50"
        )?;
        
        let rows = stmt.query_map(params![pattern], |row| {
            Ok(TorrentHistory {
                id: row.get(0)?,
                magnet_link: row.get(1)?,
                name: row.get(2)?,
                added_at: row.get(3)?,
                total_size: row.get(4)?,
                status: row.get(5)?,
            })
        })?;
        
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        
        Ok(results)
    }
}

/// Sanitize string input to prevent injection and control characters
fn sanitize_string(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .take(500) // Limit string length
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_database_operations() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db = Database::new(&db_path).unwrap();
        
        // Test adding magnet
        let id = db.add_magnet(
            "magnet:?xt=urn:btih:abc123",
            "Test Torrent"
        ).unwrap();
        assert!(id > 0);
        
        // Test duplicate handling
        let id2 = db.add_magnet(
            "magnet:?xt=urn:btih:abc123",
            "Test Torrent"
        ).unwrap();
        assert_eq!(id, id2);
        
        // Test history retrieval
        let history = db.get_history(None).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].name, "Test Torrent");
    }
}
