use std::fs::{self};
use tempfile::tempdir;
use tofu_core::error::TofuError;
use tofu_core::extract::{extract, ExtractOptions};
use tofu_core::pack::{pack, PackOptions};
use tofu_core::verify::verify;

fn create_valid_test_archive() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    let src_file = dir.path().join("preset.fxp");
    fs::write(&src_file, b"Cutoff=100;Resonance=50;Filter=Lowpass;").unwrap();

    let archive = dir.path().join("test.tofu");
    pack(&src_file, &archive, &PackOptions::default()).unwrap();

    (dir, archive)
}

#[test]
fn test_corrupted_magic_rejection() {
    let (_dir, archive) = create_valid_test_archive();
    let mut bytes = fs::read(&archive).unwrap();

    // Corrupt magic identifier: 'T' -> 'Z'
    bytes[0] = b'Z';
    fs::write(&archive, &bytes).unwrap();

    let err = verify(&archive, None::<fn(tofu_core::verify::VerifyProgress)>).unwrap_err();
    assert!(matches!(err, TofuError::InvalidMagic { .. }));
}

#[test]
fn test_unsupported_version_rejection() {
    let (_dir, archive) = create_valid_test_archive();
    let mut bytes = fs::read(&archive).unwrap();

    // Set major version to 99 (offset 4..6)
    bytes[4] = 99;
    bytes[5] = 0;
    fs::write(&archive, &bytes).unwrap();

    let err = verify(&archive, None::<fn(tofu_core::verify::VerifyProgress)>).unwrap_err();
    assert!(matches!(err, TofuError::UnsupportedVersion { major: 99, .. }));
}

#[test]
fn test_corrupted_header_crc_rejection() {
    let (_dir, archive) = create_valid_test_archive();
    let mut bytes = fs::read(&archive).unwrap();

    // Mutate file count in header (offset 16..24) without updating CRC
    bytes[16] ^= 0x01;
    fs::write(&archive, &bytes).unwrap();

    let err = verify(&archive, None::<fn(tofu_core::verify::VerifyProgress)>).unwrap_err();
    assert!(matches!(err, TofuError::HeaderCrcMismatch { .. }));
}

#[test]
fn test_tampered_payload_detected_by_sha256() {
    let (dir, archive) = create_valid_test_archive();
    let mut bytes = fs::read(&archive).unwrap();

    // Tamper with payload byte (payload starts at offset 80)
    bytes[85] ^= 0x55;
    fs::write(&archive, &bytes).unwrap();

    // Verification must catch CRC or SHA-256 failure
    let err = verify(&archive, None::<fn(tofu_core::verify::VerifyProgress)>).unwrap_err();
    assert!(
        matches!(
            err,
            TofuError::ChunkCrcMismatch { .. }
                | TofuError::PayloadChecksumMismatch { .. }
                | TofuError::DecompressionError { .. }
        ),
        "Expected checksum or decompression error, got {:?}",
        err
    );

    // Extraction must also refuse to extract and catch corruption
    let ext_dir = dir.path().join("extracted");
    let ext_err = extract(&archive, &ext_dir, &ExtractOptions::default()).unwrap_err();
    assert!(
        matches!(
            ext_err,
            TofuError::ChunkCrcMismatch { .. }
                | TofuError::PayloadChecksumMismatch { .. }
                | TofuError::DecompressionError { .. }
        ),
        "Expected extraction corruption error, got {:?}",
        ext_err
    );
}

#[test]
fn test_truncated_archive_handling() {
    let (_dir, archive) = create_valid_test_archive();
    let bytes = fs::read(&archive).unwrap();

    // Truncate archive in half
    let truncated_bytes = &bytes[..bytes.len() / 2];
    fs::write(&archive, truncated_bytes).unwrap();

    let err = verify(&archive, None::<fn(tofu_core::verify::VerifyProgress)>).unwrap_err();
    assert!(
        matches!(
            err,
            TofuError::TruncatedArchive { .. }
                | TofuError::CorruptedArchive(_)
                | TofuError::HeaderCrcMismatch { .. }
                | TofuError::Io(_)
        ),
        "Expected clean error on truncated archive, got {:?}",
        err
    );
}
