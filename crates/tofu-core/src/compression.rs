use crate::error::{Result, TofuError};
use crate::format::{COMPRESSION_STORE, COMPRESSION_ZSTD};
use std::io::{Read, Write};

/// Default compression level for Zstandard (balanced speed & ratio).
pub const DEFAULT_ZSTD_LEVEL: i32 = 3;

/// Buffer size for streaming I/O operations (64 KB).
pub const STREAM_BUFFER_SIZE: usize = 64 * 1024;

/// Threshold size below which we test compression to prevent negative ratio expansion.
pub const AUTO_STORE_THRESHOLD: usize = 64 * 1024;

/// Decompresses data from a stream according to the specified compression method.
///
/// Ensures exact byte streaming and does not allow decompressing more than `orig_size` bytes
/// to guard against zip-bomb / resource exhaustion attacks.
pub fn decompress_stream<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    method: u8,
    comp_size: u64,
    orig_size: u64,
) -> Result<()> {
    match method {
        COMPRESSION_STORE => {
            let mut limited_reader = reader.take(comp_size);
            let mut buffer = [0u8; STREAM_BUFFER_SIZE];
            let mut copied = 0u64;

            while copied < comp_size {
                let to_read = std::cmp::min(buffer.len() as u64, comp_size - copied) as usize;
                let n = limited_reader.read(&mut buffer[..to_read])?;
                if n == 0 {
                    break;
                }
                writer.write_all(&buffer[..n])?;
                copied += n as u64;
            }

            if copied != comp_size || copied != orig_size {
                return Err(TofuError::SizeMismatch {
                    path: "stream".to_string(),
                    expected: orig_size,
                    actual: copied,
                });
            }
            Ok(())
        }
        COMPRESSION_ZSTD => {
            let limited_reader = reader.take(comp_size);
            let mut decoder = zstd::stream::read::Decoder::new(limited_reader)
                .map_err(|e| TofuError::DecompressionError {
                    path: "stream".to_string(),
                    source: e,
                })?;

            let mut buffer = [0u8; STREAM_BUFFER_SIZE];
            let mut total_uncompressed = 0u64;

            loop {
                let n = decoder.read(&mut buffer).map_err(|e| TofuError::DecompressionError {
                    path: "stream".to_string(),
                    source: e,
                })?;
                if n == 0 {
                    break;
                }
                total_uncompressed += n as u64;

                // Zip-bomb defense: abort if decompressed stream exceeds expected original size
                if total_uncompressed > orig_size {
                    return Err(TofuError::SizeMismatch {
                        path: "stream".to_string(),
                        expected: orig_size,
                        actual: total_uncompressed,
                    });
                }
                writer.write_all(&buffer[..n])?;
            }

            if total_uncompressed != orig_size {
                return Err(TofuError::SizeMismatch {
                    path: "stream".to_string(),
                    expected: orig_size,
                    actual: total_uncompressed,
                });
            }
            Ok(())
        }
        other => Err(TofuError::UnsupportedCompression(other)),
    }
}
