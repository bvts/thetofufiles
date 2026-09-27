use std::fs::{self, File};
use std::io::Write;
use tempfile::tempdir;
use tofu_core::error::TofuError;
use tofu_core::extract::{extract, ExtractOptions};
use tofu_core::format::{Header, COMPRESSION_STORE, HEADER_SIZE};
use tofu_core::manifest::{Manifest, ManifestEntry};
use tofu_core::pack::{pack, PackOptions};

#[test]
fn test_reject_existing_destination_without_force() {
    let dir = tempdir().unwrap();
    let src_file = dir.path().join("input.txt");
    fs::write(&src_file, b"content").unwrap();

    let archive = dir.path().join("out.tofu");
    let opts = PackOptions::default();
    pack(&src_file, &archive, &opts).unwrap();

    // Packing again to the same archive without force must fail with DestinationExists
    let res = pack(&src_file, &archive, &opts);
    assert!(matches!(res, Err(TofuError::DestinationExists(_))));

    // Extracting to an existing file without force must fail
    let ext_dir = dir.path().join("extracted");
    fs::create_dir_all(&ext_dir).unwrap();
    let existing_dest_file = ext_dir.join("input.txt");
    fs::write(&existing_dest_file, b"existing").unwrap();

    let ext_opts = ExtractOptions {
        force: false,
        restore_mtime: true,
        progress_callback: None,
    };
    let ext_res = extract(&archive, &ext_dir, &ext_opts);
    assert!(matches!(ext_res, Err(TofuError::DestinationExists(_))));
}

#[test]
fn test_craft_path_traversal_archive_rejected() {
    let dir = tempdir().unwrap();
    let archive_path = dir.path().join("malicious.tofu");

    // Manually craft a malicious archive containing "../../escaped.txt"
    let mut file = File::create(&archive_path).unwrap();

    // Placeholder header
    file.write_all(&[0u8; HEADER_SIZE as usize]).unwrap();

    // Write 10 bytes payload
    let payload = b"malicious!";
    file.write_all(payload).unwrap();

    // Create manifest with illegal traversal path directly bypassing ManifestEntry validator
    let mut manifest = Manifest::new();
    manifest.entries.push(ManifestEntry {
        path: "../../escaped.txt".to_string(),
        original_size: payload.len() as u64,
        compressed_size: payload.len() as u64,
        payload_offset: HEADER_SIZE as u64,
        compression_method: COMPRESSION_STORE,
        compression_level: 0,
        mtime: 0,
        entry_flags: 0,
        sha256: tofu_core::hashing::hash_bytes(payload),
        crc32_compressed: tofu_core::hashing::crc32_bytes(payload),
    });

    let manifest_bytes = manifest.to_bytes().unwrap();
    let manifest_offset = HEADER_SIZE as u64 + payload.len() as u64;
    let manifest_len = manifest_bytes.len() as u64;
    file.write_all(&manifest_bytes).unwrap();

    // Seek to 0 and write header
    use std::io::Seek;
    file.seek(std::io::SeekFrom::Start(0)).unwrap();
    let header = Header::new(
        0,
        1,
        HEADER_SIZE as u64,
        payload.len() as u64,
        0,
        0,
        manifest_offset,
        manifest_len,
    );
    header.write_to(&mut file).unwrap();
    file.flush().unwrap();

    // Attempting to extract malicious archive must fail with PathTraversalAttempt
    let ext_target = dir.path().join("safe_dir");
    let ext_opts = ExtractOptions::default();
    let err = extract(&archive_path, &ext_target, &ext_opts).unwrap_err();

    assert!(
        matches!(err, TofuError::PathTraversalAttempt(_)),
        "Expected PathTraversalAttempt, got {:?}",
        err
    );
}

#[test]
fn test_case_insensitive_collision_detection() {
    let mut manifest = Manifest::new();
    manifest
        .add_entry(ManifestEntry {
            path: "Bass.fxp".to_string(),
            original_size: 10,
            compressed_size: 10,
            payload_offset: 80,
            compression_method: 0,
            compression_level: 0,
            mtime: 0,
            entry_flags: 0,
            sha256: [0; 32],
            crc32_compressed: 0,
        })
        .unwrap();

    manifest
        .add_entry(ManifestEntry {
            path: "bass.fxp".to_string(), // Collides on Windows/Mac
            original_size: 10,
            compressed_size: 10,
            payload_offset: 90,
            compression_method: 0,
            compression_level: 0,
            mtime: 0,
            entry_flags: 0,
            sha256: [0; 32],
            crc32_compressed: 0,
        })
        .unwrap();

    let res = manifest.check_case_collisions();
    assert!(
        matches!(res, Err(TofuError::PathCaseCollision { .. })),
        "Expected PathCaseCollision, got {:?}",
        res
    );
}

#[test]
fn test_astronomical_manifest_count_rejected() {
    // Malicious craft: manifest payload says count = u64::MAX
    let mut manifest_bytes = Vec::new();
    manifest_bytes.extend_from_slice(b"TMAN");
    
    // 8 bytes entry count: u64::MAX
    let count_bytes = u64::MAX.to_le_bytes();
    let hash = tofu_core::hashing::hash_bytes(&count_bytes);
    manifest_bytes.extend_from_slice(&hash);
    manifest_bytes.extend_from_slice(&count_bytes);

    let res = Manifest::from_bytes(&manifest_bytes);
    assert!(
        matches!(res, Err(TofuError::CorruptedArchive(_))),
        "Expected CorruptedArchive on astronomical count, got {:?}",
        res
    );
}

#[test]
fn test_declared_section_exceeding_file_length_rejected() {
    let dir = tempdir().unwrap();
    let archive_path = dir.path().join("overflow.tofu");

    // Create 80-byte file with manifest length = 100 GB
    let mut file = File::create(&archive_path).unwrap();
    let header = Header::new(
        0,
        1,
        80,
        0,
        0,
        0,
        80,
        100_000_000_000, // 100 GB
    );
    header.write_to(&mut file).unwrap();
    file.flush().unwrap();

    let ext_dir = dir.path().join("ext");
    let res = extract(&archive_path, &ext_dir, &ExtractOptions::default());
    assert!(
        matches!(res, Err(TofuError::CorruptedArchive(_))),
        "Expected CorruptedArchive when section exceeds file length, got {:?}",
        res
    );

    let insp_res = tofu_core::inspect(&archive_path);
    assert!(
        matches!(insp_res, Err(TofuError::CorruptedArchive(_))),
        "Expected CorruptedArchive in inspect, got {:?}",
        insp_res
    );
}

