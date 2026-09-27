use crate::error::{Result, TofuError};
use crate::hashing::hash_bytes;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

pub const METADATA_MAGIC: [u8; 4] = *b"TMET";

/// Extensible archive package metadata.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,

    // Music & Preset Specific Fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_version: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset_count: Option<u64>,

    // Arbitrary extensible fields
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl Metadata {
    pub fn new() -> Self {
        Self::default()
    }

    /// Serializes metadata into a binary block: `TMET` (4 bytes) + SHA-256 (32 bytes) + JSON bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let json_bytes = serde_json::to_vec_pretty(self)
            .map_err(|e| TofuError::MetadataError(e.to_string()))?;

        let sha256 = hash_bytes(&json_bytes);

        let mut buf = Vec::with_capacity(4 + 32 + json_bytes.len());
        buf.write_all(&METADATA_MAGIC)?;
        buf.write_all(&sha256)?;
        buf.write_all(&json_bytes)?;
        Ok(buf)
    }

    /// Parses and verifies metadata from a binary block.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 36 {
            return Err(TofuError::TruncatedArchive {
                section: "metadata header",
                needed: 36,
                available: bytes.len(),
            });
        }

        let mut cursor = Cursor::new(bytes);
        let mut magic = [0u8; 4];
        cursor.read_exact(&mut magic)?;

        if magic != METADATA_MAGIC {
            return Err(TofuError::CorruptedArchive(format!(
                "Invalid metadata magic: expected 'TMET', found {magic:?}"
            )));
        }

        let mut expected_sha256 = [0u8; 32];
        cursor.read_exact(&mut expected_sha256)?;

        let json_bytes = &bytes[36..];
        let calculated_sha256 = hash_bytes(json_bytes);

        if calculated_sha256 != expected_sha256 {
            return Err(TofuError::CorruptedArchive(format!(
                "Metadata SHA-256 mismatch: expected {}, calculated {}",
                hex::encode(expected_sha256),
                hex::encode(calculated_sha256)
            )));
        }

        let metadata: Metadata = serde_json::from_slice(json_bytes)
            .map_err(|e| TofuError::MetadataError(e.to_string()))?;

        Ok(metadata)
    }
}
