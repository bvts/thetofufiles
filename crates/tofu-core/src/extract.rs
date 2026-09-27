use crate::compression::decompress_stream;
use crate::error::{Result, TofuError};
use crate::format::Header;
use crate::hashing::Sha256Stream;
use crate::manifest::Manifest;
use std::fs::{self, File, FileTimes};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

/// Progress event emitted during extraction.
pub enum ExtractProgress<'a> {
    FileStart { path: &'a str, index: usize, total: usize },
    FileComplete { path: &'a str, bytes: u64 },
}

pub type ExtractProgressCallback<'a> = Box<dyn Fn(ExtractProgress) + 'a>;

/// Options configuring archive extraction.
pub struct ExtractOptions<'a> {
    pub force: bool,
    pub restore_mtime: bool,
    pub progress_callback: Option<ExtractProgressCallback<'a>>,
}

impl<'a> Default for ExtractOptions<'a> {
    fn default() -> Self {
        Self {
            force: false,
            restore_mtime: true,
            progress_callback: None,
        }
    }
}

/// Statistics reported after extraction.
#[derive(Debug, Clone)]
pub struct ExtractStats {
    pub file_count: u64,
    pub bytes_extracted: u64,
}

/// Ensures an output path is strictly contained within the target extraction root directory.
fn sanitize_extract_path(target_root: &Path, rel_path: &str) -> Result<PathBuf> {
    Manifest::validate_path(rel_path)?;

    let clean = rel_path.replace('\\', "/");
    let mut dest = target_root.to_path_buf();

    for seg in clean.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            return Err(TofuError::PathTraversalAttempt(format!(
                "Path traversal '..' segment detected: '{rel_path}'"
            )));
        }
        dest.push(seg);
    }

    if !dest.starts_with(target_root) {
        return Err(TofuError::PathTraversalAttempt(format!(
            "Path escapes target directory: '{rel_path}'"
        )));
    }

    Ok(dest)
}

/// A writer wrapper that calculates SHA-256 while writing to a destination file.
struct HashingFileWriter {
    inner: File,
    hasher: Sha256Stream,
    bytes_written: u64,
}

impl HashingFileWriter {
    fn new(inner: File) -> Self {
        Self {
            inner,
            hasher: Sha256Stream::new(),
            bytes_written: 0,
        }
    }

    fn finalize(self) -> (File, [u8; 32], u64) {
        (self.inner, self.hasher.finalize(), self.bytes_written)
    }
}

impl Write for HashingFileWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        self.bytes_written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Extracts all contents of a `.tofu` archive into the target directory.
pub fn extract<P: AsRef<Path>, Q: AsRef<Path>>(
    archive_path: P,
    target_dir: Q,
    options: &ExtractOptions,
) -> Result<ExtractStats> {
    let archive_p = archive_path.as_ref();
    let target = target_dir.as_ref();

    if !target.exists() {
        fs::create_dir_all(target)?;
    }
    let canonical_root = target.canonicalize().map_err(TofuError::Io)?;

    let mut archive_file = File::open(archive_p)?;
    let archive_len = archive_file.metadata()?.len();

    let header = Header::read_from(&mut archive_file)?;

    // Validate archive boundaries before allocating buffers
    if header.manifest_offset.saturating_add(header.manifest_length) > archive_len {
        return Err(TofuError::CorruptedArchive(format!(
            "Manifest boundary exceeds file size: {} + {} > {archive_len}",
            header.manifest_offset, header.manifest_length
        )));
    }

    if header.payload_offset.saturating_add(header.payload_length) > archive_len {
        return Err(TofuError::CorruptedArchive(format!(
            "Payload boundary exceeds file size: {} + {} > {archive_len}",
            header.payload_offset, header.payload_length
        )));
    }

    // Read and verify manifest
    archive_file.seek(SeekFrom::Start(header.manifest_offset))?;
    let mut manifest_bytes = vec![0u8; header.manifest_length as usize];
    archive_file.read_exact(&mut manifest_bytes).map_err(|_| {
        TofuError::TruncatedArchive {
            section: "manifest",
            needed: header.manifest_length as usize,
            available: 0,
        }
    })?;

    let manifest = Manifest::from_bytes(&manifest_bytes)?;

    let mut bytes_extracted = 0u64;
    let total_entries = manifest.entries.len();

    // Buffer reader for streaming archive reads
    let mut reader = BufReader::new(archive_file);

    for (idx, entry) in manifest.entries.iter().enumerate() {
        if let Some(cb) = &options.progress_callback {
            cb(ExtractProgress::FileStart {
                path: &entry.path,
                index: idx + 1,
                total: total_entries,
            });
        }

        let dest_path = sanitize_extract_path(&canonical_root, &entry.path)?;

        if entry.is_directory() {
            fs::create_dir_all(&dest_path)?;
            continue;
        }

        if let Some(parent) = dest_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        if dest_path.exists() && !options.force {
            return Err(TofuError::DestinationExists(dest_path));
        }

        // Seek directly to payload slice for this file
        reader.seek(SeekFrom::Start(entry.payload_offset))?;

        let out_file = File::create(&dest_path)?;
        let mut hashing_writer = HashingFileWriter::new(out_file);

        // Streaming decompress directly to disk
        let decompress_res = decompress_stream(
            &mut reader,
            &mut hashing_writer,
            entry.compression_method,
            entry.compressed_size,
            entry.original_size,
        );

        if let Err(e) = decompress_res {
            let _ = fs::remove_file(&dest_path);
            return Err(e);
        }

        hashing_writer.flush()?;
        let (file, computed_sha256, written) = hashing_writer.finalize();

        // Strict cryptographic lossless verification
        if computed_sha256 != entry.sha256 {
            let _ = fs::remove_file(&dest_path);
            return Err(TofuError::PayloadChecksumMismatch {
                path: entry.path.clone(),
                expected: hex::encode(entry.sha256),
                calculated: hex::encode(computed_sha256),
            });
        }

        if written != entry.original_size {
            let _ = fs::remove_file(&dest_path);
            return Err(TofuError::SizeMismatch {
                path: entry.path.clone(),
                expected: entry.original_size,
                actual: written,
            });
        }

        // Restore file modification time if requested
        if options.restore_mtime && entry.mtime > 0 {
            let mtime_system = UNIX_EPOCH + Duration::from_secs(entry.mtime);
            let times = FileTimes::new().set_modified(mtime_system);
            let _ = file.set_times(times);
        }

        bytes_extracted += written;

        if let Some(cb) = &options.progress_callback {
            cb(ExtractProgress::FileComplete {
                path: &entry.path,
                bytes: written,
            });
        }
    }

    Ok(ExtractStats {
        file_count: manifest.entries.len() as u64,
        bytes_extracted,
    })
}
