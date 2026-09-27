use std::path::PathBuf;
use thiserror::Error;

/// Result type alias for TOFU operations.
pub type Result<T> = std::result::Result<T, TofuError>;

/// Errors that can occur during TOFU container operations.
#[derive(Error, Debug)]
pub enum TofuError {
    #[error("Invalid TOFU magic identifier: expected {expected:?}, found {found:?}")]
    InvalidMagic {
        expected: [u8; 4],
        found: [u8; 4],
    },

    #[error("Unsupported TOFU format version {major}.{minor} (this engine supports v1.x)")]
    UnsupportedVersion {
        major: u16,
        minor: u16,
    },

    #[error("Header CRC-32 checksum mismatch: expected {expected:#010x}, calculated {calculated:#010x}")]
    HeaderCrcMismatch {
        expected: u32,
        calculated: u32,
    },

    #[error("Invalid manifest magic identifier: expected {expected:?}, found {found:?}")]
    ManifestMagicMismatch {
        expected: [u8; 4],
        found: [u8; 4],
    },

    #[error("Manifest SHA-256 integrity check failed: expected {expected}, calculated {calculated}")]
    ManifestSha256Mismatch {
        expected: String,
        calculated: String,
    },

    #[error("Compressed chunk CRC-32 mismatch for '{path}': expected {expected:#010x}, calculated {calculated:#010x}")]
    ChunkCrcMismatch {
        path: String,
        expected: u32,
        calculated: u32,
    },

    #[error("Lossless payload SHA-256 mismatch for '{path}': expected {expected}, calculated {calculated}")]
    PayloadChecksumMismatch {
        path: String,
        expected: String,
        calculated: String,
    },

    #[error("File size mismatch for '{path}': expected {expected} bytes, found {actual} bytes")]
    SizeMismatch {
        path: String,
        expected: u64,
        actual: u64,
    },

    #[error("Path traversal attack detected: '{0}' attempts to escape target directory")]
    PathTraversalAttempt(String),

    #[error("Case-insensitive path collision: '{original}' clashes with '{colliding}'")]
    PathCaseCollision {
        original: String,
        colliding: String,
    },

    #[error("Invalid entry path: '{0}'")]
    InvalidPath(String),

    #[error("Archive is truncated in section '{section}': needed {needed} bytes, but only {available} available")]
    TruncatedArchive {
        section: &'static str,
        needed: usize,
        available: usize,
    },

    #[error("Corrupted archive structure: {0}")]
    CorruptedArchive(String),

    #[error("Decompression failed for '{path}': {source}")]
    DecompressionError {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("Compression error: {0}")]
    CompressionError(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Metadata serialization / parsing error: {0}")]
    MetadataError(String),

    #[error("Destination directory or file already exists: {0}")]
    DestinationExists(PathBuf),

    #[error("Input directory or file is empty")]
    EmptyInput,

    #[error("Unsupported compression algorithm id: {0}")]
    UnsupportedCompression(u8),
}
