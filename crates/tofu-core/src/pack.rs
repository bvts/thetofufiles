use crate::compression::{AUTO_STORE_THRESHOLD, DEFAULT_ZSTD_LEVEL, STREAM_BUFFER_SIZE};
use crate::error::{Result, TofuError};
use crate::format::{
    COMPRESSION_STORE, COMPRESSION_ZSTD, ENTRY_FLAG_DIRECTORY, FLAG_HAS_METADATA, HEADER_SIZE,
    Header,
};
use crate::hashing::{crc32_bytes, hash_bytes, HashingWriter, Sha256Stream};
use crate::manifest::{Manifest, ManifestEntry};
use crate::metadata::Metadata;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

/// Progress event emitted during archive packing.
pub enum PackProgress<'a> {
    FileStart { path: &'a str, index: usize, total: usize },
    FileComplete {
        path: &'a str,
        original_size: u64,
        compressed_size: u64,
        method: u8,
    },
}

pub type PackProgressCallback<'a> = Box<dyn Fn(PackProgress) + 'a>;

/// Options configuring the packing process.
pub struct PackOptions<'a> {
    pub compression_level: i32,
    pub force: bool,
    pub metadata: Option<Metadata>,
    pub progress_callback: Option<PackProgressCallback<'a>>,
}

impl<'a> Default for PackOptions<'a> {
    fn default() -> Self {
        Self {
            compression_level: DEFAULT_ZSTD_LEVEL,
            force: false,
            metadata: None,
            progress_callback: None,
        }
    }
}

/// Statistics reported after successfully packing an archive.
#[derive(Debug, Clone)]
pub struct PackStats {
    pub file_count: u64,
    pub total_original_size: u64,
    pub total_compressed_size: u64,
    pub archive_size: u64,
}

impl PackStats {
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
            (1.0 - (self.total_compressed_size as f64 / self.total_original_size as f64)) * 100.0
        }
    }
}

struct ItemToPack {
    relative_path: String,
    disk_path: PathBuf,
    is_directory: bool,
    mtime: u64,
}

/// Recursively scans an input path and collects relative items to archive.
fn collect_items(input_path: &Path, exclude_output: Option<&Path>) -> Result<Vec<ItemToPack>> {
    let mut items = Vec::new();

    if input_path.is_file() {
        let file_name = input_path
            .file_name()
            .ok_or_else(|| TofuError::InvalidPath(input_path.display().to_string()))?
            .to_string_lossy();
        let normalized = Manifest::normalize_path(&file_name)?;

        let metadata = fs::metadata(input_path)?;
        let mtime = metadata
            .modified()
            .unwrap_or_else(|_| SystemTime::now())
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        items.push(ItemToPack {
            relative_path: normalized,
            disk_path: input_path.to_path_buf(),
            is_directory: false,
            mtime,
        });
        return Ok(items);
    }

    if !input_path.exists() {
        return Err(TofuError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Path not found: {}", input_path.display()),
        )));
    }

    // Traverse directory
    for entry_res in WalkDir::new(input_path).sort_by_file_name() {
        let entry = entry_res.map_err(|e| {
            TofuError::Io(std::io::Error::other(format!(
                "Failed to read directory entry: {e}"
            )))
        })?;

        let path = entry.path();
        if path == input_path {
            continue; // Skip the root directory itself
        }

        // Never pack the target output file inside the archive
        if let Some(out_p) = exclude_output {
            if path == out_p {
                continue;
            }
            if let (Ok(p_canon), Ok(out_canon)) = (path.canonicalize(), out_p.canonicalize()) {
                if p_canon == out_canon {
                    continue;
                }
            }
        }

        let rel = path.strip_prefix(input_path).map_err(|_| {
            TofuError::InvalidPath(format!("Failed to strip prefix from {}", path.display()))
        })?;

        let rel_str = rel.to_string_lossy();
        let normalized = Manifest::normalize_path(&rel_str)?;

        let metadata = entry.metadata().map_err(|e| {
            TofuError::Io(std::io::Error::other(format!(
                "Failed to read metadata for {}: {e}",
                path.display()
            )))
        })?;

        let mtime = metadata
            .modified()
            .unwrap_or_else(|_| SystemTime::now())
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let is_dir = metadata.is_dir();

        items.push(ItemToPack {
            relative_path: normalized,
            disk_path: path.to_path_buf(),
            is_directory: is_dir,
            mtime,
        });
    }

    Ok(items)
}

/// Packs an input directory or file into a `.tofu` archive.
pub fn pack<P: AsRef<Path>, Q: AsRef<Path>>(
    input_path: P,
    output_path: Q,
    options: &PackOptions,
) -> Result<PackStats> {
    let input = input_path.as_ref();
    let output = output_path.as_ref();

    if output.exists() && !options.force {
        return Err(TofuError::DestinationExists(output.to_path_buf()));
    }

    let items = collect_items(input, Some(output))?;

    // Check for case collisions before writing any archive data
    let mut temp_manifest = Manifest::new();
    for it in &items {
        temp_manifest.add_entry(ManifestEntry {
            path: it.relative_path.clone(),
            original_size: 0,
            compressed_size: 0,
            payload_offset: 0,
            compression_method: 0,
            compression_level: 0,
            mtime: it.mtime,
            entry_flags: if it.is_directory { ENTRY_FLAG_DIRECTORY } else { 0 },
            sha256: [0u8; 32],
            crc32_compressed: 0,
        })?;
    }
    temp_manifest.check_case_collisions()?;

    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    let file = File::create(output)?;
    let mut writer = BufWriter::with_capacity(STREAM_BUFFER_SIZE, file);

    // 1. Write placeholder header (80 bytes)
    let placeholder_header = [0u8; HEADER_SIZE as usize];
    writer.write_all(&placeholder_header)?;

    let mut manifest = Manifest::new();
    let mut total_original_size = 0u64;
    let mut total_compressed_size = 0u64;
    let total_items = items.len();

    let payload_offset = HEADER_SIZE as u64;
    let mut current_offset = payload_offset;

    // 2. Stream compress file payloads
    for (idx, item) in items.iter().enumerate() {
        if let Some(cb) = &options.progress_callback {
            cb(PackProgress::FileStart {
                path: &item.relative_path,
                index: idx + 1,
                total: total_items,
            });
        }

        if item.is_directory {
            manifest.add_entry(ManifestEntry {
                path: item.relative_path.clone(),
                original_size: 0,
                compressed_size: 0,
                payload_offset: current_offset,
                compression_method: COMPRESSION_STORE,
                compression_level: 0,
                mtime: item.mtime,
                entry_flags: ENTRY_FLAG_DIRECTORY,
                sha256: [0u8; 32],
                crc32_compressed: 0,
            })?;
            continue;
        }

        let file_meta = fs::metadata(&item.disk_path)?;
        let orig_size = file_meta.len();

        let (method, level, comp_size, sha256, crc32) = if orig_size <= AUTO_STORE_THRESHOLD as u64 {
            // Memory buffer test for small presets (auto-store fallback)
            let mut file = File::open(&item.disk_path)?;
            let mut uncompressed = Vec::with_capacity(orig_size as usize);
            file.read_to_end(&mut uncompressed)?;

            let sha256 = hash_bytes(&uncompressed);

            if options.compression_level > 0 && !uncompressed.is_empty() {
                let mut compressed = Vec::new();
                let mut encoder = zstd::stream::write::Encoder::new(
                    &mut compressed,
                    options.compression_level,
                )
                .map_err(|e| TofuError::CompressionError(e.to_string()))?;
                encoder.write_all(&uncompressed)?;
                encoder.finish().map_err(|e| TofuError::CompressionError(e.to_string()))?;

                if compressed.len() < uncompressed.len() {
                    let crc32 = crc32_bytes(&compressed);
                    writer.write_all(&compressed)?;
                    (
                        COMPRESSION_ZSTD,
                        options.compression_level as i16,
                        compressed.len() as u64,
                        sha256,
                        crc32,
                    )
                } else {
                    // Stored fallback: compression did not save space on this small preset!
                    let crc32 = crc32_bytes(&uncompressed);
                    writer.write_all(&uncompressed)?;
                    (COMPRESSION_STORE, 0, orig_size, sha256, crc32)
                }
            } else {
                let crc32 = crc32_bytes(&uncompressed);
                writer.write_all(&uncompressed)?;
                (COMPRESSION_STORE, 0, orig_size, sha256, crc32)
            }
        } else {
            // Streaming I/O for large audio sample files: 0 RAM bloat
            let mut file = BufReader::with_capacity(STREAM_BUFFER_SIZE, File::open(&item.disk_path)?);

            if options.compression_level > 0 {
                let mut hashing_writer = HashingWriter::new(&mut writer);
                let mut sha_hasher = Sha256Stream::new();

                {
                    let mut encoder = zstd::stream::write::Encoder::new(
                        &mut hashing_writer,
                        options.compression_level,
                    )
                    .map_err(|e| TofuError::CompressionError(e.to_string()))?;

                    let mut buf = [0u8; STREAM_BUFFER_SIZE];
                    loop {
                        let n = file.read(&mut buf)?;
                        if n == 0 {
                            break;
                        }
                        sha_hasher.update(&buf[..n]);
                        encoder.write_all(&buf[..n])?;
                    }
                    encoder.finish().map_err(|e| TofuError::CompressionError(e.to_string()))?;
                }

                let sha256 = sha_hasher.finalize();
                let crc32 = hashing_writer.crc32();
                let comp_size = hashing_writer.bytes_written();

                (
                    COMPRESSION_ZSTD,
                    options.compression_level as i16,
                    comp_size,
                    sha256,
                    crc32,
                )
            } else {
                let mut hashing_writer = HashingWriter::new(&mut writer);
                let mut sha_hasher = Sha256Stream::new();
                let mut buf = [0u8; STREAM_BUFFER_SIZE];

                loop {
                    let n = file.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    sha_hasher.update(&buf[..n]);
                    hashing_writer.write_all(&buf[..n])?;
                }

                let sha256 = sha_hasher.finalize();
                let crc32 = hashing_writer.crc32();
                let comp_size = hashing_writer.bytes_written();

                (COMPRESSION_STORE, 0, comp_size, sha256, crc32)
            }
        };

        manifest.add_entry(ManifestEntry {
            path: item.relative_path.clone(),
            original_size: orig_size,
            compressed_size: comp_size,
            payload_offset: current_offset,
            compression_method: method,
            compression_level: level,
            mtime: item.mtime,
            entry_flags: 0,
            sha256,
            crc32_compressed: crc32,
        })?;

        total_original_size += orig_size;
        total_compressed_size += comp_size;
        current_offset += comp_size;

        if let Some(cb) = &options.progress_callback {
            cb(PackProgress::FileComplete {
                path: &item.relative_path,
                original_size: orig_size,
                compressed_size: comp_size,
                method,
            });
        }
    }

    let payload_length = current_offset - payload_offset;

    // 3. Write Metadata section if present
    let (metadata_offset, metadata_length, flags) = if let Some(meta) = &options.metadata {
        let meta_bytes = meta.to_bytes()?;
        let offset = current_offset;
        let len = meta_bytes.len() as u64;
        writer.write_all(&meta_bytes)?;
        current_offset += len;
        (offset, len, FLAG_HAS_METADATA)
    } else {
        (0, 0, 0)
    };

    // 4. Write Manifest section
    let manifest_bytes = manifest.to_bytes()?;
    let manifest_offset = current_offset;
    let manifest_length = manifest_bytes.len() as u64;
    writer.write_all(&manifest_bytes)?;
    current_offset += manifest_length;

    // 5. Seek back to byte 0 and write finalized Header
    writer.flush()?;
    let mut file = writer.into_inner().map_err(|e| e.into_error())?;
    file.seek(SeekFrom::Start(0))?;

    let header = Header::new(
        flags,
        manifest.entries.len() as u64,
        payload_offset,
        payload_length,
        metadata_offset,
        metadata_length,
        manifest_offset,
        manifest_length,
    );
    header.write_to(&mut file)?;
    file.flush()?;

    Ok(PackStats {
        file_count: manifest.entries.len() as u64,
        total_original_size,
        total_compressed_size,
        archive_size: current_offset,
    })
}
