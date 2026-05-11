use std::collections::HashSet;
use std::time::Instant;

use crate::delivery::Envelope;

/// On-chain registry caps a single index_batch at this many CIDs.
/// Must stay in sync with chronicle_registry_core::MAX_BATCH.
pub const MAX_BATCH: usize = 50;

/// A single entry waiting to be anchored.
#[derive(Debug, Clone)]
pub struct PendingEntry {
    pub cid: String,
    pub metadata_hash: [u8; 32],
    pub timestamp: u64,
}

/// In-memory accumulator. Holds entries until a flush condition fires.
pub struct BatchBuffer {
    entries: Vec<PendingEntry>,
    first_entry_at: Option<Instant>,
    max_size: usize,
    flush_after_s: u64,
    /// CIDs known to be anchored on-chain (seeded at startup, updated on flush).
    pub known: HashSet<String>,
}

impl BatchBuffer {
    pub fn new(max_size: usize, flush_after_s: u64, known: HashSet<String>) -> Self {
        Self {
            entries: Vec::new(),
            first_entry_at: None,
            max_size,
            flush_after_s,
            known,
        }
    }

    /// Try to add an envelope. Silently skips if the CID is already known or
    /// already pending in this buffer.
    pub fn push(&mut self, env: Envelope) {
        if self.known.contains(&env.cid) {
            return;
        }
        if self.entries.iter().any(|e| e.cid == env.cid) {
            return;
        }
        if self.first_entry_at.is_none() {
            self.first_entry_at = Some(Instant::now());
        }
        self.entries.push(PendingEntry {
            cid: env.cid,
            metadata_hash: env.metadata_hash,
            timestamp: env.timestamp,
        });
    }

    /// True when the buffer should be flushed.
    pub fn should_flush(&self) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        if self.entries.len() >= self.max_size {
            return true;
        }
        if let Some(first) = self.first_entry_at {
            return first.elapsed().as_secs() >= self.flush_after_s;
        }
        false
    }

    /// Drain entries ready to submit. Caller is responsible for calling
    /// `mark_flushed` on success so the dedup set is updated.
    pub fn drain(&mut self) -> Vec<PendingEntry> {
        self.first_entry_at = None;
        std::mem::take(&mut self.entries)
    }

    /// Move a successfully submitted batch into the known set.
    pub fn mark_flushed(&mut self, entries: &[PendingEntry]) {
        for e in entries {
            self.known.insert(e.cid.clone());
        }
    }

    /// On failed submit, put entries back at the front so they retry next tick.
    pub fn return_failed(&mut self, mut entries: Vec<PendingEntry>) {
        if self.first_entry_at.is_none() && !entries.is_empty() {
            self.first_entry_at = Some(Instant::now());
        }
        entries.append(&mut self.entries);
        self.entries = entries;
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn env(cid: &str) -> Envelope {
        Envelope { cid: cid.to_string(), metadata_hash: [0u8; 32], timestamp: 1 }
    }

    #[test]
    fn skips_known_cids() {
        let known = HashSet::from(["bafy1".to_string()]);
        let mut buf = BatchBuffer::new(50, 30, known);
        buf.push(env("bafy1"));
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn skips_in_buffer_duplicates() {
        let mut buf = BatchBuffer::new(50, 30, HashSet::new());
        buf.push(env("bafy1"));
        buf.push(env("bafy1"));
        assert_eq!(buf.len(), 1);
    }

    #[test]
    fn flushes_at_max_size() {
        let mut buf = BatchBuffer::new(3, 9999, HashSet::new());
        buf.push(env("a"));
        buf.push(env("b"));
        assert!(!buf.should_flush());
        buf.push(env("c"));
        assert!(buf.should_flush());
    }

    #[test]
    fn drain_clears_entries() {
        let mut buf = BatchBuffer::new(50, 30, HashSet::new());
        buf.push(env("a"));
        let drained = buf.drain();
        assert_eq!(drained.len(), 1);
        assert!(buf.is_empty());
    }

    #[test]
    fn mark_flushed_updates_known_set() {
        let mut buf = BatchBuffer::new(50, 30, HashSet::new());
        buf.push(env("a"));
        let batch = buf.drain();
        buf.mark_flushed(&batch);
        // Now "a" is known — pushing again is a no-op
        buf.push(env("a"));
        assert!(buf.is_empty());
    }

    #[test]
    fn return_failed_puts_entries_back() {
        let mut buf = BatchBuffer::new(50, 30, HashSet::new());
        buf.push(env("a"));
        let batch = buf.drain();
        assert!(buf.is_empty());
        buf.return_failed(batch);
        assert_eq!(buf.len(), 1);
    }
}
