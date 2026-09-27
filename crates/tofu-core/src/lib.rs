//! # TOFU Core Library
//!
//! A custom, lossless binary container format engineered specifically for
//! music-production presets, sample libraries, and plugin assets.
//!
//! Core guarantee: **Any file packed into a `.tofu` archive is recoverable
//! byte-for-byte identically after extraction.**

pub mod compression;
pub mod error;
pub mod extract;
pub mod format;
pub mod hashing;
pub mod manifest;
pub mod metadata;
pub mod pack;
pub mod verify;

pub use error::{Result, TofuError};
pub use extract::{extract, ExtractOptions, ExtractProgress, ExtractStats};
pub use format::{Header, FORMAT_VERSION_MAJOR, FORMAT_VERSION_MINOR, HEADER_SIZE, TOFU_MAGIC};
pub use manifest::{Manifest, ManifestEntry};
pub use metadata::Metadata;
pub use pack::{pack, PackOptions, PackProgress, PackStats};
pub use verify::{verify, VerifyProgress, VerifyReport};

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const SOFTWARE_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const FORMAT_VERSION_STR: &str = "1.0";

/// Detailed archive summary for display in `tofu info`.
#[derive(Debug, Clone)]
pub struct ArchiveInfo {
    pub header: Header,
    pub metadata: Option<Metadata>,
    pub manifest: Manifest,
    pub archive_size: u64,
    pub total_original_size: u64,
    pub total_compressed_size: u64,
}

impl ArchiveInfo {
    pub fn compression_ratio(&self) -> f64 {
        if self.total_original_size == 0 {
            1.0
        } else {
            self.total_compressed_size as f64 / self.total_original_size as f64
        }
    }

    pub fn space_saved_percent(&self) -> f64 {
        if self.total_original_size == 0 {
            0.0
        } else {
            (1.0 - self.compression_ratio()) * 100.0
        }
    }
}

/// Reads archive information (header, manifest, metadata) without extracting or decompressing payloads.
pub fn inspect<P: AsRef<Path>>(archive_path: P) -> Result<ArchiveInfo> {
    let mut file = File::open(archive_path.as_ref())?;
    let archive_size = file.metadata()?.len();

    let header = Header::read_from(&mut file)?;

    if header.manifest_offset.saturating_add(header.manifest_length) > archive_size {
        return Err(TofuError::CorruptedArchive(format!(
            "Manifest boundary exceeds file size: {} + {} > {archive_size}",
            header.manifest_offset, header.manifest_length
        )));
    }

    if header.metadata_length > 0
        && header.metadata_offset.saturating_add(header.metadata_length) > archive_size
    {
        return Err(TofuError::CorruptedArchive(format!(
            "Metadata boundary exceeds file size: {} + {} > {archive_size}",
            header.metadata_offset, header.metadata_length
        )));
    }

    let metadata = if header.metadata_length > 0 {
        file.seek(SeekFrom::Start(header.metadata_offset))?;
        let mut meta_bytes = vec![0u8; header.metadata_length as usize];
        file.read_exact(&mut meta_bytes)?;
        Some(Metadata::from_bytes(&meta_bytes)?)
    } else {
        None
    };

    file.seek(SeekFrom::Start(header.manifest_offset))?;
    let mut manifest_bytes = vec![0u8; header.manifest_length as usize];
    file.read_exact(&mut manifest_bytes)?;
    let manifest = Manifest::from_bytes(&manifest_bytes)?;

    let mut total_original_size = 0u64;
    let mut total_compressed_size = 0u64;

    for entry in &manifest.entries {
        total_original_size += entry.original_size;
        total_compressed_size += entry.compressed_size;
    }

    Ok(ArchiveInfo {
        header,
        metadata,
        manifest,
        archive_size,
        total_original_size,
        total_compressed_size,
    })
}

/// Lists all contained files and directories in the archive.
pub fn list<P: AsRef<Path>>(archive_path: P) -> Result<Vec<ManifestEntry>> {
    let info = inspect(archive_path)?;
    Ok(info.manifest.entries)
}
