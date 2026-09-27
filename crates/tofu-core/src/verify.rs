use crate::compression::decompress_stream;
use crate::error::{Result, TofuError};
use crate::format::Header;
use crate::hashing::{crc32_bytes, Sha256Stream};
use crate::manifest::Manifest;
use crate::metadata::Metadata;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Progress event emitted during verification.
pub enum VerifyProgress<'a> {
    HeaderOk,
    ManifestOk { count: usize },
    MetadataOk,
    FileStart { path: &'a str, index: usize, total: usize },
    FileOk { path: &'a str },
}

/// A writer sink that discards bytes while computing SHA-256 digest.
struct HashingSink {
    hasher: Sha256Stream,
    bytes_count: u64,
}

impl HashingSink {
    fn new() -> Self {
        Self {
            hasher: Sha256Stream::new(),
            bytes_count: 0,
        }
    }

    fn finalize(self) -> ([u8; 32], u64) {
        (self.hasher.finalize(), self.bytes_count)
    }
}

impl Write for HashingSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.hasher.update(buf);
        self.bytes_count += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Comprehensive verification report for an archive.
#[derive(Debug, Clone)]
pub struct VerifyReport {
    pub file_count: u64,
    pub total_original_size: u64,
    pub total_compressed_size: u64,
    pub has_metadata: bool,
    pub is_valid: bool,
    pub details: Vec<String>,
}

/// Verifies archive integrity, manifest, and every compressed payload without writing to disk.
pub fn verify<P: AsRef<Path>, F>(
    archive_path: P,
    progress_callback: Option<F>,
) -> Result<VerifyReport>
where
    F: Fn(VerifyProgress),
{
    let path = archive_path.as_ref();
    let mut file = File::open(path)?;
    let archive_len = file.metadata()?.len();

    // 1. Verify Header and CRC
    let header = Header::read_from(&mut file)?;
    if let Some(ref cb) = progress_callback {
        cb(VerifyProgress::HeaderOk);
    }

    // Sanity checks on header offsets
    if header.payload_offset + header.payload_length > archive_len {
        return Err(TofuError::CorruptedArchive(format!(
            "Payload boundary extends past archive size: {} > {}",
            header.payload_offset + header.payload_length,
            archive_len
        )));
    }

    if header.manifest_offset + header.manifest_length > archive_len {
        return Err(TofuError::CorruptedArchive(format!(
            "Manifest boundary extends past archive size: {} > {}",
            header.manifest_offset + header.manifest_length,
            archive_len
        )));
    }

    // 2. Verify Metadata if present
    let mut has_metadata = false;
    if header.metadata_length > 0 {
        if header.metadata_offset + header.metadata_length > archive_len {
            return Err(TofuError::CorruptedArchive(
                "Metadata boundary extends past archive size".to_string(),
            ));
        }

        file.seek(SeekFrom::Start(header.metadata_offset))?;
        let mut meta_bytes = vec![0u8; header.metadata_length as usize];
        file.read_exact(&mut meta_bytes)?;
        Metadata::from_bytes(&meta_bytes)?;
        has_metadata = true;

        if let Some(ref cb) = progress_callback {
            cb(VerifyProgress::MetadataOk);
        }
    }

    // 3. Verify Manifest
    file.seek(SeekFrom::Start(header.manifest_offset))?;
    let mut manifest_bytes = vec![0u8; header.manifest_length as usize];
    file.read_exact(&mut manifest_bytes)?;
    let manifest = Manifest::from_bytes(&manifest_bytes)?;

    if let Some(ref cb) = progress_callback {
        cb(VerifyProgress::ManifestOk {
            count: manifest.entries.len(),
        });
    }

    let mut total_original_size = 0u64;
    let mut total_compressed_size = 0u64;
    let total_entries = manifest.entries.len();
    let mut details = Vec::new();

    let mut reader = BufReader::new(file);

    // 4. Stream-verify each payload chunk
    for (idx, entry) in manifest.entries.iter().enumerate() {
        if let Some(ref cb) = progress_callback {
            cb(VerifyProgress::FileStart {
                path: &entry.path,
                index: idx + 1,
                total: total_entries,
            });
        }

        if entry.is_directory() {
            details.push(format!("DIR  {}", entry.path));
            continue;
        }

        if entry.payload_offset + entry.compressed_size > archive_len {
            return Err(TofuError::CorruptedArchive(format!(
                "Payload for '{}' extends past archive size",
                entry.path
            )));
        }

        reader.seek(SeekFrom::Start(entry.payload_offset))?;

        // 4a. Quick check: verify compressed chunk CRC-32
        let mut comp_bytes = vec![0u8; entry.compressed_size as usize];
        reader.read_exact(&mut comp_bytes)?;
        let computed_crc = crc32_bytes(&comp_bytes);
        if computed_crc != entry.crc32_compressed {
            return Err(TofuError::ChunkCrcMismatch {
                path: entry.path.clone(),
                expected: entry.crc32_compressed,
                calculated: computed_crc,
            });
        }

        // 4b. Deep check: stream decompress and verify lossless SHA-256
        let mut cursor = std::io::Cursor::new(comp_bytes);
        let mut sink = HashingSink::new();

        decompress_stream(
            &mut cursor,
            &mut sink,
            entry.compression_method,
            entry.compressed_size,
            entry.original_size,
        )?;

        let (computed_sha256, uncompressed_len) = sink.finalize();

        if computed_sha256 != entry.sha256 {
            return Err(TofuError::PayloadChecksumMismatch {
                path: entry.path.clone(),
                expected: hex::encode(entry.sha256),
                calculated: hex::encode(computed_sha256),
            });
        }

        if uncompressed_len != entry.original_size {
            return Err(TofuError::SizeMismatch {
                path: entry.path.clone(),
                expected: entry.original_size,
                actual: uncompressed_len,
            });
        }

        total_original_size += entry.original_size;
        total_compressed_size += entry.compressed_size;
        details.push(format!("OK   {} (SHA-256 verified)", entry.path));

        if let Some(ref cb) = progress_callback {
            cb(VerifyProgress::FileOk { path: &entry.path });
        }
    }

    Ok(VerifyReport {
        file_count: manifest.entries.len() as u64,
        total_original_size,
        total_compressed_size,
        has_metadata,
        is_valid: true,
        details,
    })
}
