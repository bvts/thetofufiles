# TOFU Container Format Specification (v1.0)

**Format Extension:** `.tofu`  
**MIME Type (proposed):** `application/x-tofu-preset-container`  
**Current Version:** 1.0  
**Byte Ordering:** Little-Endian (all integers, counts, offsets, and checksums)

---

## 1. Overview & Guarantees

TOFU is a custom, deterministic binary container designed for distributing music-production presets, sample packs, and plugin soundbanks.

### Core Guarantee
> **Any file packed into a `.tofu` archive must be recoverable byte-for-byte identically after extraction.**

The format enforces this guarantee cryptographically:
1. Every file entry stores an uncompressed **SHA-256** digest (`[u8; 32]`).
2. During extraction and verification, the decompressed payload stream is hashed on-the-fly and checked against the manifest digest.
3. If a single bit differs, extraction aborts and the corrupt file is deleted immediately.

---

## 2. High-Level Archive Layout

A `.tofu` file consists of four sequential sections:

```text
+-------------------------------------------------------------------+
| Section 1: Header (Fixed 80 bytes)                                |
+-------------------------------------------------------------------+
| Section 2: Payloads (Sequential compressed/stored byte streams)   |
|   ├── Payload Chunk 0                                             |
|   ├── Payload Chunk 1                                             |
|   └── ...                                                         |
+-------------------------------------------------------------------+
| Section 3: Metadata Block (Optional, extensible JSON + SHA-256)   |
+-------------------------------------------------------------------+
| Section 4: Manifest Block (TMAN catalog + SHA-256)                |
+-------------------------------------------------------------------+
```

Because each file in the manifest contains its absolute `payload_offset` within the archive, readers can seek directly to any file in **$O(1)$ time** without decompressing the surrounding archive.

---

## 3. Section 1: Header (80 Bytes)

The header is fixed at **80 bytes** (`0x50` bytes), 64-bit aligned.

| Offset (Hex) | Field Name | Type | Size | Description |
|---|---|---|---|---|
| `0x00..0x04` | `magic` | `[u8; 4]` | 4 B | ASCII `"TOFU"` (`0x54, 0x4F, 0x46, 0x55`) |
| `0x04..0x06` | `version_major` | `u16` LE | 2 B | Major version (`1`) |
| `0x06..0x08` | `version_minor` | `u16` LE | 2 B | Minor version (`0`) |
| `0x08..0x0C` | `flags` | `u32` LE | 4 B | Bitflags (see Header Flags below) |
| `0x0C..0x10` | `header_size` | `u32` LE | 4 B | Size of header in bytes (`80` / `0x50`) |
| `0x10..0x18` | `file_count` | `u64` LE | 8 B | Total number of items (files & directories) |
| `0x18..0x20` | `payload_offset` | `u64` LE | 8 B | Byte offset where payloads begin (`80`) |
| `0x20..0x28` | `payload_length` | `u64` LE | 8 B | Total byte length of all payload chunks |
| `0x28..0x30` | `metadata_offset`| `u64` LE | 8 B | Byte offset of metadata block (or 0 if none) |
| `0x30..0x38` | `metadata_length`| `u64` LE | 8 B | Length of metadata block in bytes (or 0) |
| `0x38..0x40` | `manifest_offset`| `u64` LE | 8 B | Byte offset where the manifest table begins |
| `0x40..0x48` | `manifest_length`| `u64` LE | 8 B | Total byte length of the manifest table |
| `0x48..0x4C` | `header_crc` | `u32` LE | 4 B | CRC-32 of bytes `0x00..0x48` |
| `0x4C..0x50` | `reserved` | `[u8; 4]` | 4 B | Reserved for future alignment (must be 0) |

### Header Flags
- `0x00000001` (`FLAG_HAS_METADATA`): Indicates that the archive contains a valid metadata section.
- `0x00000002` (`FLAG_DEDUPLICATED`): Reserved for future multi-path payload deduplication.
- `0xFFFFFFFC`: Reserved bits (must be 0 in v1.0).

---

## 4. Section 2: Payload Chunks

Payloads are stored consecutively starting at `payload_offset` (`80`).

Each file's payload is either:
- **Zstandard Compressed (`compression_method = 1`)**: Standard Zstandard stream (`libzstd`).
- **Stored (`compression_method = 0`)**: Raw uncompressed bytes.

### Auto-Store Fallback Rule
If a file's compressed size is greater than or equal to its uncompressed size (e.g. tiny 1KB XML synth presets or already-compressed FLAC/PNG assets), TOFU automatically stores it uncompressed (`compression_method = 0`). This prevents negative compression expansion.

---

## 5. Section 3: Metadata Block (Optional)

When `metadata_length > 0`, the metadata block begins at `metadata_offset`:

| Field Name | Type | Description |
|---|---|---|
| `magic` | `[u8; 4]` | ASCII `"TMET"` (`0x54, 0x4D, 0x45, 0x54`) |
| `sha256` | `[u8; 32]` | SHA-256 digest of the following JSON byte buffer |
| `json_data` | `[u8]` | UTF-8 encoded JSON object |

### Standard JSON Schema
```json
{
  "name": "Serum Cinematic Vol. 1",
  "author": "Sound Designer",
  "description": "50 expressive cinematic presets for Serum 2",
  "version": "1.0.0",
  "created_at": "1774720000",
  "plugin": "Serum 2",
  "plugin_version": "2.0.1",
  "category": "Cinematic",
  "tags": ["Pad", "Atmosphere", "Drone"],
  "preset_count": 50
}
```

---

## 6. Section 4: Manifest Block

The manifest is the authoritative table of contents:

| Field Name | Type | Description |
|---|---|---|
| `magic` | `[u8; 4]` | ASCII `"TMAN"` (`0x54, 0x4D, 0x41, 0x4E`) |
| `sha256` | `[u8; 32]` | Cryptographic SHA-256 digest of all following entries |
| `entry_count` | `u64` LE | Number of catalog entries |

### Manifest Entry Structure (Repeated `entry_count` times)

| Field Name | Type | Size | Description |
|---|---|---|---|
| `path_len` | `u16` LE | 2 B | Byte length of `path_bytes` (max 65,535) |
| `path_bytes` | `[u8]` | `path_len` | Normalized UTF-8 relative path (forward slashes `/`) |
| `original_size` | `u64` LE | 8 B | Uncompressed size in bytes |
| `compressed_size` | `u64` LE | 8 B | Compressed byte length in payload slice |
| `payload_offset` | `u64` LE | 8 B | Absolute byte offset in `.tofu` file |
| `compression_method` | `u8` | 1 B | `0` = Store, `1` = Zstd |
| `compression_level` | `i16` LE | 2 B | Zstd compression level (e.g. 3, 19, or 0) |
| `mtime` | `u64` LE | 8 B | Unix modification timestamp in seconds |
| `entry_flags` | `u16` LE | 2 B | `0x0001` = Directory, `0x0002` = Read Only |
| `sha256` | `[u8; 32]` | 32 B | Cryptographic SHA-256 of original uncompressed bytes |
| `crc32_compressed` | `u32` LE | 4 B | CRC-32 checksum of the compressed chunk |

---

## 7. Security & Integrity Validation

All compliant TOFU readers and extractors must enforce:

1. **Path Traversal Defenses**:
   - Reject any path beginning with `/`, `\`, or Windows drive letters (`C:`).
   - Reject path segments containing `..` or null characters `\0`.
   - Normalization converts all `\` into `/`.
   - Verify that resolved extraction paths reside strictly inside the target root directory.
2. **Case-Insensitive Collision Defense**:
   - Check and reject archives containing files that differ only by case on case-insensitive filesystems (e.g. NTFS on Windows, APFS on macOS).
3. **Zip-Bomb & Resource Exhaustion Defense**:
   - Decompress directly to disk using bounded buffers.
   - Enforce streaming limit: abort extraction if uncompressed stream exceeds `original_size`.
4. **Non-Panicking Degradation**:
   - Truncated files or corrupted checksums must yield clean, typed errors with non-zero exit codes.
