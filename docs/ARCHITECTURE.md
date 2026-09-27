# TOFU Architecture & System Design

This document details the software architecture, data flow, and design decisions behind the TOFU lossless audio preset container.

---

## 1. Subsystem Architecture

The project is structured as a modular Cargo workspace:

```text
tofu/
├── Cargo.toml                  # Workspace root
├── crates/
│   ├── tofu-core/              # Headless library crate (engine & format)
│   │   ├── format.rs           # Header and binary layout definitions
│   │   ├── compression.rs      # Streaming Zstd & Auto-Store compression engine
│   │   ├── hashing.rs          # Streaming SHA-256 and CRC-32 tee-writers
│   │   ├── manifest.rs         # Catalog serialization & path sanitization
│   │   ├── metadata.rs         # JSON package metadata serialization
│   │   ├── pack.rs             # Streaming directory walker & archive packer
│   │   ├── extract.rs          # Safe streaming extractor with rollback
│   │   ├── verify.rs           # In-memory streaming integrity verifier
│   │   └── error.rs            # Typed error enumerations (TofuError)
│   └── tofu-cli/               # CLI binary crate (tofu.exe)
│       ├── main.rs             # Clap command line parsing & execution
│       └── ...
└── tests/                      # Integration and robustness test suites
```

### Decoupling for Future GUI Integration
`tofu-core` has **zero UI dependencies** and compiles into a lightweight Rust library. A future desktop application built with Tauri, Slint, egui, or C-FFI can link directly to `tofu-core` without modifying a single line of format or extraction logic.

---

## 2. Data Flow

### A. Packing Data Flow (`tofu pack`)

```text
[Input Files / Dirs]
         │
         ▼
[Item Collector & Path Normalizer]
         │
         ├──> Case-Collision Detector (Fails early if "Lead.fxp" and "lead.fxp" collide)
         │
         ▼
[Write 80-Byte Placeholder Header to Archive]
         │
         ▼
[For Each File] ──> [Hash SHA-256 of Uncompressed Stream]
         │
         ├── Size <= 64KB ? ──> [Test Zstd Compression]
         │                           ├── Smaller? ──> Write Zstd Payload Slice
         │                           └── Larger?  ──> Fallback to Raw Store Mode
         │
         └── Size > 64KB  ? ──> [Stream Zstd Encoder / Pass-through directly to disk]
                                     └── Write Compressed Chunk CRC-32
         │
         ▼
[Write Metadata Block] (TMET + SHA-256 + JSON)
         │
         ▼
[Write Manifest Block] (TMAN + SHA-256 + File Catalog)
         │
         ▼
[Seek Back to Byte 0]
         │
         ▼
[Write Finalized 80-byte Header with Header CRC-32]
```

### B. Extraction Data Flow (`tofu extract`)

```text
[Open .tofu File]
         │
         ├──> Read & Verify 80-Byte Header (Check CRC-32)
         ├──> Seek to Manifest Offset & Authenticate Manifest SHA-256
         │
         ▼
[For Each Manifest Entry]
         │
         ├──> Sanitize Relative Path (Reject .., drive letters, absolute paths)
         ├──> Check if Destination Exists (Abort unless --force)
         ├──> Seek Directly to payload_offset
         │
         ▼
[Stream Decompress Directly to Destination File]
         │
         ├──> Calculate Uncompressed SHA-256 on the fly
         ├──> Enforce Streaming Bound (Abort if decompressed bytes > original_size)
         │
         ▼
[Integrity Gate]
         ├── Hashes Match? ──> Restore original mtime & Keep File
         └── Mismatch?     ──> Delete Partial File & Abort with TofuError
```

### C. In-Memory Verification (`tofu verify`)

`tofu verify` uses the exact same cryptographic pipeline as extraction, but directs decompressed bytes into an in-memory hashing sink (`HashingSink`), guaranteeing **zero disk I/O**.

---

## 3. Key Design Decisions

1. **Deterministic Endianness**: All binary fields are strictly Little-Endian (`byteorder::LittleEndian`), ensuring identical archive byte representations across x86_64, ARM64, and Apple Silicon.
2. **Fixed-Size Header with Offsets**: Rather than requiring a sequential uncompressed scan (tar-style) or a trailing footer that breaks on truncated downloads (zip-style), TOFU uses an 80-byte header that explicitly points to `payload_offset`, `metadata_offset`, and `manifest_offset`.
3. **Random Access ($O(1)$)**: Because every file records its own `payload_offset` and `compressed_size`, individual presets can be extracted in milliseconds without decompressing the rest of the archive.
4. **Auto-Store Fallback**: Prevents archive bloat on tiny XML/JSON presets by falling back to uncompressed storage if Zstandard compression produces negative space savings.
5. **Strict Path Canonicalization**: Defends against directory traversal attacks (`../../`), ensuring extraction never writes outside the specified directory.
