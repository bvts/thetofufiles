use crc32fast::Hasher as Crc32Hasher;
use sha2::{Digest, Sha256};
use std::io::{self, Write};

/// Streaming SHA-256 digest accumulator.
#[derive(Default, Clone)]
pub struct Sha256Stream {
    hasher: Sha256,
    bytes_count: u64,
}

impl Sha256Stream {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, data: &[u8]) {
        self.hasher.update(data);
        self.bytes_count += data.len() as u64;
    }

    pub fn bytes_hashed(&self) -> u64 {
        self.bytes_count
    }

    pub fn finalize(self) -> [u8; 32] {
        self.hasher.finalize().into()
    }

    pub fn finalize_hex(self) -> String {
        hex::encode(self.finalize())
    }
}

/// A writer wrapper that calculates SHA-256 and CRC-32 while transparently forwarding writes.
pub struct HashingWriter<W: Write> {
    inner: W,
    sha256: Sha256Stream,
    crc32: Crc32Hasher,
    bytes_written: u64,
}

impl<W: Write> HashingWriter<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            sha256: Sha256Stream::new(),
            crc32: Crc32Hasher::new(),
            bytes_written: 0,
        }
    }

    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }

    pub fn crc32(&self) -> u32 {
        self.crc32.clone().finalize()
    }

    pub fn sha256_digest(&self) -> [u8; 32] {
        self.sha256.clone().finalize()
    }

    pub fn into_inner(self) -> (W, [u8; 32], u32, u64) {
        (
            self.inner,
            self.sha256.finalize(),
            self.crc32.finalize(),
            self.bytes_written,
        )
    }
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.sha256.update(&buf[..n]);
        self.crc32.update(&buf[..n]);
        self.bytes_written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Computes the SHA-256 hash of a byte slice.
pub fn hash_bytes(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// Computes the CRC-32 of a byte slice.
pub fn crc32_bytes(data: &[u8]) -> u32 {
    let mut hasher = Crc32Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hashing_writer() {
        let mut buffer = Vec::new();
        let mut writer = HashingWriter::new(&mut buffer);

        let data1 = b"Hello, ";
        let data2 = b"TOFU audio container!";
        writer.write_all(data1).unwrap();
        writer.write_all(data2).unwrap();
        writer.flush().unwrap();

        assert_eq!(writer.bytes_written(), (data1.len() + data2.len()) as u64);

        let full_data = b"Hello, TOFU audio container!";
        assert_eq!(writer.sha256_digest(), hash_bytes(full_data));
        assert_eq!(writer.crc32(), crc32_bytes(full_data));
    }
}
