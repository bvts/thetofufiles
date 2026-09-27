use crate::error::{Result, TofuError};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use crc32fast::Hasher as Crc32Hasher;
use std::io::{Read, Write};

/// Magic 4-byte identifier at the start of every TOFU container.
pub const TOFU_MAGIC: [u8; 4] = *b"TOFU";

/// Magic 4-byte identifier for the manifest block.
pub const MANIFEST_MAGIC: [u8; 4] = *b"TMAN";

/// Current format major version.
pub const FORMAT_VERSION_MAJOR: u16 = 1;

/// Current format minor version.
pub const FORMAT_VERSION_MINOR: u16 = 0;

/// Fixed byte size of the TOFU v1 header.
pub const HEADER_SIZE: u32 = 80;

/// Header Flag: Archive contains a metadata section.
pub const FLAG_HAS_METADATA: u32 = 1 << 0;

/// Header Flag: Reserved for future deduplication.
pub const FLAG_DEDUPLICATED: u32 = 1 << 1;

/// Compression Method: Stored (raw, uncompressed).
pub const COMPRESSION_STORE: u8 = 0;

/// Compression Method: Zstandard (lossless).
pub const COMPRESSION_ZSTD: u8 = 1;

/// Entry Flag: Item is a directory.
pub const ENTRY_FLAG_DIRECTORY: u16 = 1 << 0;

/// Entry Flag: Item is read-only.
pub const ENTRY_FLAG_READONLY: u16 = 1 << 1;

/// Represents the fixed-size 80-byte header of a `.tofu` archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub magic: [u8; 4],
    pub version_major: u16,
    pub version_minor: u16,
    pub flags: u32,
    pub header_size: u32,
    pub file_count: u64,
    pub payload_offset: u64,
    pub payload_length: u64,
    pub metadata_offset: u64,
    pub metadata_length: u64,
    pub manifest_offset: u64,
    pub manifest_length: u64,
    pub header_crc: u32,
    pub reserved: [u8; 4],
}

impl Header {
    /// Create a new Header for v1.0 with given dimensions and offsets.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        flags: u32,
        file_count: u64,
        payload_offset: u64,
        payload_length: u64,
        metadata_offset: u64,
        metadata_length: u64,
        manifest_offset: u64,
        manifest_length: u64,
    ) -> Self {
        let mut header = Self {
            magic: TOFU_MAGIC,
            version_major: FORMAT_VERSION_MAJOR,
            version_minor: FORMAT_VERSION_MINOR,
            flags,
            header_size: HEADER_SIZE,
            file_count,
            payload_offset,
            payload_length,
            metadata_offset,
            metadata_length,
            manifest_offset,
            manifest_length,
            header_crc: 0,
            reserved: [0u8; 4],
        };
        header.header_crc = header.compute_crc();
        header
    }

    /// Compute CRC-32 for the first 72 bytes of the header (excluding CRC and reserved).
    pub fn compute_crc(&self) -> u32 {
        let mut buf = Vec::with_capacity(72);
        buf.extend_from_slice(&self.magic);
        buf.extend_from_slice(&self.version_major.to_le_bytes());
        buf.extend_from_slice(&self.version_minor.to_le_bytes());
        buf.extend_from_slice(&self.flags.to_le_bytes());
        buf.extend_from_slice(&self.header_size.to_le_bytes());
        buf.extend_from_slice(&self.file_count.to_le_bytes());
        buf.extend_from_slice(&self.payload_offset.to_le_bytes());
        buf.extend_from_slice(&self.payload_length.to_le_bytes());
        buf.extend_from_slice(&self.metadata_offset.to_le_bytes());
        buf.extend_from_slice(&self.metadata_length.to_le_bytes());
        buf.extend_from_slice(&self.manifest_offset.to_le_bytes());
        buf.extend_from_slice(&self.manifest_length.to_le_bytes());

        let mut hasher = Crc32Hasher::new();
        hasher.update(&buf);
        hasher.finalize()
    }

    /// Serialize header to an 80-byte array.
    pub fn to_bytes(&self) -> [u8; 80] {
        let mut buf = [0u8; 80];
        let mut cursor = &mut buf[..];
        cursor.write_all(&self.magic).unwrap();
        cursor.write_u16::<LittleEndian>(self.version_major).unwrap();
        cursor.write_u16::<LittleEndian>(self.version_minor).unwrap();
        cursor.write_u32::<LittleEndian>(self.flags).unwrap();
        cursor.write_u32::<LittleEndian>(self.header_size).unwrap();
        cursor.write_u64::<LittleEndian>(self.file_count).unwrap();
        cursor.write_u64::<LittleEndian>(self.payload_offset).unwrap();
        cursor.write_u64::<LittleEndian>(self.payload_length).unwrap();
        cursor.write_u64::<LittleEndian>(self.metadata_offset).unwrap();
        cursor.write_u64::<LittleEndian>(self.metadata_length).unwrap();
        cursor.write_u64::<LittleEndian>(self.manifest_offset).unwrap();
        cursor.write_u64::<LittleEndian>(self.manifest_length).unwrap();
        cursor.write_u32::<LittleEndian>(self.header_crc).unwrap();
        cursor.write_all(&self.reserved).unwrap();
        buf
    }

    /// Parse and validate header from a byte buffer.
    pub fn from_bytes(bytes: &[u8; 80]) -> Result<Self> {
        let mut cursor = &bytes[..];

        let mut magic = [0u8; 4];
        cursor.read_exact(&mut magic).map_err(|_| TofuError::TruncatedArchive {
            section: "header",
            needed: 80,
            available: bytes.len(),
        })?;

        if magic != TOFU_MAGIC {
            return Err(TofuError::InvalidMagic {
                expected: TOFU_MAGIC,
                found: magic,
            });
        }

        let version_major = cursor.read_u16::<LittleEndian>()?;
        let version_minor = cursor.read_u16::<LittleEndian>()?;

        if version_major != FORMAT_VERSION_MAJOR {
            return Err(TofuError::UnsupportedVersion {
                major: version_major,
                minor: version_minor,
            });
        }

        let flags = cursor.read_u32::<LittleEndian>()?;
        let header_size = cursor.read_u32::<LittleEndian>()?;
        let file_count = cursor.read_u64::<LittleEndian>()?;
        let payload_offset = cursor.read_u64::<LittleEndian>()?;
        let payload_length = cursor.read_u64::<LittleEndian>()?;
        let metadata_offset = cursor.read_u64::<LittleEndian>()?;
        let metadata_length = cursor.read_u64::<LittleEndian>()?;
        let manifest_offset = cursor.read_u64::<LittleEndian>()?;
        let manifest_length = cursor.read_u64::<LittleEndian>()?;
        let header_crc = cursor.read_u32::<LittleEndian>()?;

        let mut reserved = [0u8; 4];
        cursor.read_exact(&mut reserved)?;

        let header = Self {
            magic,
            version_major,
            version_minor,
            flags,
            header_size,
            file_count,
            payload_offset,
            payload_length,
            metadata_offset,
            metadata_length,
            manifest_offset,
            manifest_length,
            header_crc,
            reserved,
        };

        let calculated_crc = header.compute_crc();
        if calculated_crc != header_crc {
            return Err(TofuError::HeaderCrcMismatch {
                expected: header_crc,
                calculated: calculated_crc,
            });
        }

        Ok(header)
    }

    /// Read and validate header directly from a reader.
    pub fn read_from<R: Read>(reader: &mut R) -> Result<Self> {
        let mut buf = [0u8; 80];
        reader.read_exact(&mut buf).map_err(|e| {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                TofuError::TruncatedArchive {
                    section: "header",
                    needed: 80,
                    available: 0,
                }
            } else {
                TofuError::Io(e)
            }
        })?;
        Self::from_bytes(&buf)
    }

    /// Write header directly to a writer.
    pub fn write_to<W: Write>(&self, writer: &mut W) -> Result<()> {
        let bytes = self.to_bytes();
        writer.write_all(&bytes)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_roundtrip() {
        let header = Header::new(
            FLAG_HAS_METADATA,
            42,
            80,
            1024,
            1104,
            256,
            1360,
            512,
        );

        let bytes = header.to_bytes();
        assert_eq!(bytes.len(), 80);

        let parsed = Header::from_bytes(&bytes).expect("Failed to parse valid header");
        assert_eq!(header, parsed);
    }

    #[test]
    fn test_header_corrupted_magic() {
        let header = Header::new(0, 1, 80, 10, 0, 0, 90, 50);
        let mut bytes = header.to_bytes();
        bytes[0] = b'X';
        let err = Header::from_bytes(&bytes).unwrap_err();
        match err {
            TofuError::InvalidMagic { .. } => (),
            _ => panic!("Expected InvalidMagic, got {:?}", err),
        }
    }

    #[test]
    fn test_header_corrupted_crc() {
        let header = Header::new(0, 1, 80, 10, 0, 0, 90, 50);
        let mut bytes = header.to_bytes();
        bytes[10] ^= 0xFF; // Mutate flags
        let err = Header::from_bytes(&bytes).unwrap_err();
        match err {
            TofuError::HeaderCrcMismatch { .. } => (),
            _ => panic!("Expected HeaderCrcMismatch, got {:?}", err),
        }
    }
}
