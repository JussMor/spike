//! Content-addressed blob store.
//!
//! Layout:  <root>/blobs/<first-2-chars>/<rest-of-hash>
//!
//! Identical to Git's loose object layout, minus the zlib compression.
//! Reads are O(1) by hash; writes are atomic (write-temp, rename).

use crate::error::{Result, VcsError};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    pub fn new(root: &Path) -> Result<Self> {
        let blobs = root.join("blobs");
        fs::create_dir_all(&blobs)?;
        Ok(Self { root: blobs })
    }

    /// Store bytes, return their BLAKE3 hex hash.
    pub fn put(&self, data: &[u8]) -> Result<String> {
        let hash = blake3_hex(data);
        let path = self.path_for(&hash);
        if path.exists() {
            return Ok(hash); // content-addressed: already there
        }
        fs::create_dir_all(path.parent().unwrap())?;
        // Write to a temp file then rename for atomicity
        let tmp = path.with_extension("tmp");
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(data)?;
            f.sync_all()?;
        }
        fs::rename(&tmp, &path)?;
        Ok(hash)
    }

    /// Fetch bytes by BLAKE3 hex hash.
    pub fn get(&self, hash: &str) -> Result<Vec<u8>> {
        let path = self.path_for(hash);
        fs::read(&path).map_err(|_| VcsError::BlobNotFound(hash.to_owned()))
    }

    /// True if the blob exists.
    pub fn has(&self, hash: &str) -> bool {
        self.path_for(hash).exists()
    }

    fn path_for(&self, hash: &str) -> PathBuf {
        let (prefix, rest) = hash.split_at(2);
        self.root.join(prefix).join(rest)
    }
}

/// Compute a BLAKE3 hex digest of arbitrary bytes.
pub fn blake3_hex(data: &[u8]) -> String {
    blake3::hash(data).to_hex().to_string()
}
