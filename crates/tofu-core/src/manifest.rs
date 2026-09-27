use crate::error::{Result, TofuError};
use crate::format::{ENTRY_FLAG_DIRECTORY, MANIFEST_MAGIC};
use crate::hashing::hash_bytes;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

/// An individual file or directory record inside the TOFU manifest table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    /// Normalized relative path (always forward-slashed, UTF-8).
    pub path: String,
    /// Uncompressed original byte size.
    pub original_size: u64,
    /// Compressed byte size in payload.
    pub compressed_size: u64,
    /// Absolute byte offset in the archive where this file's payload begins.
    pub payload_offset: u64,
    /// Compression algorithm (0 = Store, 1 = Zstd).
    pub compression_method: u8,
    /// Compression level (e.g. 3, 19).
    pub compression_level: i16,
    /// Unix modification timestamp in seconds.
    pub mtime: u64,
    /// Flags (e.g. ENTRY_FLAG_DIRECTORY, ENTRY_FLAG_READONLY).
    pub entry_flags: u16,
    /// Cryptographic SHA-256 of original uncompressed bytes.
    pub sha256: [u8; 32],
    /// CRC-32 checksum of the compressed chunk for fast bit-rot detection.
    pub crc32_compressed: u32,
}

impl ManifestEntry {
    pub fn is_directory(&self) -> bool {
        (self.entry_flags & ENTRY_FLAG_DIRECTORY) != 0
    }

    pub fn sha256_hex(&self) -> String {
        hex::encode(self.sha256)
    }
}

/// The complete manifest catalog of all archived files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    pub entries: Vec<ManifestEntry>,
}

impl Manifest {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn add_entry(&mut self, entry: ManifestEntry) -> Result<()> {
        Self::validate_path(&entry.path)?;
        self.entries.push(entry);
        Ok(())
    }

    /// Validates that a path is strictly relative and contains no traversal escapes.
    pub fn validate_path(path: &str) -> Result<()> {
        if path.is_empty() {
            return Err(TofuError::InvalidPath("Path cannot be empty".to_string()));
        }

        if path.contains('\0') {
            return Err(TofuError::InvalidPath("Path contains null byte".to_string()));
        }

        // Reject Windows drive letters (e.g. "C:...") or alternate data streams (e.g. "file:stream")
        if path.contains(':') {
            return Err(TofuError::PathTraversalAttempt(format!(
                "Path contains invalid drive colon or ADS: '{path}'"
            )));
        }

        // Reject absolute paths
        if path.starts_with('/') || path.starts_with('\\') {
            return Err(TofuError::PathTraversalAttempt(format!(
                "Absolute paths are forbidden: '{path}'"
            )));
        }

        // Split by either separator to inspect all segments
        let segments: Vec<&str> = path.split(['/', '\\']).collect();
        for seg in segments {
            if seg == ".." {
                return Err(TofuError::PathTraversalAttempt(format!(
                    "Path traversal '..' segment detected: '{path}'"
                )));
            }
        }

        Ok(())
    }

    /// Normalizes a filesystem path to a clean, canonical forward-slash archive path.
    pub fn normalize_path(raw: &str) -> Result<String> {
        Self::validate_path(raw)?;
        let clean = raw.replace('\\', "/");
        let parts: Vec<&str> = clean.split('/').filter(|s| !s.is_empty() && *s != ".").collect();
        if parts.is_empty() {
            return Err(TofuError::InvalidPath(raw.to_string()));
        }
        Ok(parts.join("/"))
    }

    /// Checks if any entries collide in a case-insensitive manner.
    pub fn check_case_collisions(&self) -> Result<()> {
        let mut seen = HashMap::new();
        for entry in &self.entries {
            let lower = entry.path.to_lowercase();
            if let Some(existing) = seen.insert(lower, &entry.path) {
                return Err(TofuError::PathCaseCollision {
                    original: existing.clone(),
                    colliding: entry.path.clone(),
                });
            }
        }
        Ok(())
    }

    /// Serializes the entire manifest table into bytes with `TMAN` magic and SHA-256 verification.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut entries_buf = Vec::new();

        // Write entry count (u64 LE)
        entries_buf.write_u64::<LittleEndian>(self.entries.len() as u64)?;

        for entry in &self.entries {
            let path_bytes = entry.path.as_bytes();
            if path_bytes.len() > u16::MAX as usize {
                return Err(TofuError::InvalidPath(format!(
                    "Path exceeds 65535 bytes: '{}'",
                    entry.path
                )));
            }

            entries_buf.write_u16::<LittleEndian>(path_bytes.len() as u16)?;
            entries_buf.write_all(path_bytes)?;
            entries_buf.write_u64::<LittleEndian>(entry.original_size)?;
            entries_buf.write_u64::<LittleEndian>(entry.compressed_size)?;
            entries_buf.write_u64::<LittleEndian>(entry.payload_offset)?;
            entries_buf.write_u8(entry.compression_method)?;
            entries_buf.write_i16::<LittleEndian>(entry.compression_level)?;
            entries_buf.write_u64::<LittleEndian>(entry.mtime)?;
            entries_buf.write_u16::<LittleEndian>(entry.entry_flags)?;
            entries_buf.write_all(&entry.sha256)?;
            entries_buf.write_u32::<LittleEndian>(entry.crc32_compressed)?;
        }

        let sha256 = hash_bytes(&entries_buf);

        let mut manifest_bytes = Vec::with_capacity(4 + 32 + entries_buf.len());
        manifest_bytes.write_all(&MANIFEST_MAGIC)?;
        manifest_bytes.write_all(&sha256)?;
        manifest_bytes.write_all(&entries_buf)?;

        Ok(manifest_bytes)
    }

    /// Parses and verifies a manifest table from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 36 {
            return Err(TofuError::TruncatedArchive {
                section: "manifest header",
                needed: 36,
                available: bytes.len(),
            });
        }

        let mut cursor = Cursor::new(bytes);

        let mut magic = [0u8; 4];
        cursor.read_exact(&mut magic)?;
        if magic != MANIFEST_MAGIC {
            return Err(TofuError::ManifestMagicMismatch {
                expected: MANIFEST_MAGIC,
                found: magic,
            });
        }

        let mut expected_sha256 = [0u8; 32];
        cursor.read_exact(&mut expected_sha256)?;

        let entries_data = &bytes[36..];
        let calculated_sha256 = hash_bytes(entries_data);

        if calculated_sha256 != expected_sha256 {
            return Err(TofuError::ManifestSha256Mismatch {
                expected: hex::encode(expected_sha256),
                calculated: hex::encode(calculated_sha256),
            });
        }

        let mut entries_cursor = Cursor::new(entries_data);
        let count = entries_cursor.read_u64::<LittleEndian>()?;

        // Bounded allocation defense: each entry requires at least 76 bytes
        // (path_len:2, path:1, orig:8, comp:8, offset:8, method:1, lvl:2, mtime:8, flags:2, sha:32, crc:4 = 76 bytes)
        const MIN_ENTRY_BYTES: usize = 76;
        if count as usize > entries_data.len() / MIN_ENTRY_BYTES {
            return Err(TofuError::CorruptedArchive(format!(
                "Manifest declared count {count} exceeds maximum possible entries for payload size {}",
                entries_data.len()
            )));
        }

        let initial_cap = std::cmp::min(count as usize, 65536);
        let mut entries = Vec::with_capacity(initial_cap);

        for _ in 0..count {
            let path_len = entries_cursor.read_u16::<LittleEndian>()? as usize;
            let mut path_bytes = vec![0u8; path_len];
            entries_cursor.read_exact(&mut path_bytes)?;

            let path = String::from_utf8(path_bytes)
                .map_err(|_| TofuError::InvalidPath("Non-UTF8 path in manifest".to_string()))?;

            Self::validate_path(&path)?;

            let original_size = entries_cursor.read_u64::<LittleEndian>()?;
            let compressed_size = entries_cursor.read_u64::<LittleEndian>()?;
            let payload_offset = entries_cursor.read_u64::<LittleEndian>()?;
            let compression_method = entries_cursor.read_u8()?;
            let compression_level = entries_cursor.read_i16::<LittleEndian>()?;
            let mtime = entries_cursor.read_u64::<LittleEndian>()?;
            let entry_flags = entries_cursor.read_u16::<LittleEndian>()?;

            let mut sha256 = [0u8; 32];
            entries_cursor.read_exact(&mut sha256)?;

            let crc32_compressed = entries_cursor.read_u32::<LittleEndian>()?;

            entries.push(ManifestEntry {
                path,
                original_size,
                compressed_size,
                payload_offset,
                compression_method,
                compression_level,
                mtime,
                entry_flags,
                sha256,
                crc32_compressed,
            });
        }

        let manifest = Self { entries };
        manifest.check_case_collisions()?;
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_roundtrip() {
        let mut manifest = Manifest::new();
        manifest
            .add_entry(ManifestEntry {
                path: "Serum/Leads/Lead 01.fxp".to_string(),
                original_size: 4096,
                compressed_size: 1024,
                payload_offset: 80,
                compression_method: 1,
                compression_level: 3,
                mtime: 1700000000,
                entry_flags: 0,
                sha256: [0xAA; 32],
                crc32_compressed: 0x12345678,
            })
            .unwrap();

        let bytes = manifest.to_bytes().unwrap();
        let parsed = Manifest::from_bytes(&bytes).unwrap();
        assert_eq!(manifest, parsed);
    }

    #[test]
    fn test_path_traversal_detection() {
        assert!(Manifest::validate_path("../../etc/passwd").is_err());
        assert!(Manifest::validate_path("foo/../../bar").is_err());
        assert!(Manifest::validate_path("/absolute/path").is_err());
        assert!(Manifest::validate_path("C:\\Windows\\System32").is_err());
        assert!(Manifest::validate_path("valid/sub/file.fxp").is_ok());
    }

    #[test]
    fn test_manifest_tampering() {
        let mut manifest = Manifest::new();
        manifest
            .add_entry(ManifestEntry {
                path: "valid.wav".to_string(),
                original_size: 100,
                compressed_size: 50,
                payload_offset: 80,
                compression_method: 1,
                compression_level: 3,
                mtime: 0,
                entry_flags: 0,
                sha256: [1; 32],
                crc32_compressed: 100,
            })
            .unwrap();

        let mut bytes = manifest.to_bytes().unwrap();
        let last_idx = bytes.len() - 1;
        bytes[last_idx] ^= 0xFF; // Tamper with entry data

        let res = Manifest::from_bytes(&bytes);
        assert!(matches!(res, Err(TofuError::ManifestSha256Mismatch { .. })));
    }
}
