use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_cli_help_and_version() {
    let mut cmd = Command::cargo_bin("tofu").unwrap();
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("TOFU — Lossless Container"));

    let mut ver_subcmd = Command::cargo_bin("tofu").unwrap();
    ver_subcmd
        .arg("version")
        .assert()
        .success()
        .stdout(predicate::str::contains("TOFU 1.0.0"))
        .stdout(predicate::str::contains("TOFU Format v1.0"));
}

#[test]
fn test_cli_smart_pack_and_unpack_default_workflow() {
    let dir = tempdir().unwrap();
    let presets_folder = dir.path().join("MyPresets");
    fs::create_dir_all(&presets_folder).unwrap();

    let file1 = presets_folder.join("Pluck 01.fxp");
    let file2 = presets_folder.join("Bass 02.fxp");
    fs::write(&file1, b"Pluck synth patch bytes").unwrap();
    fs::write(&file2, b"Bass synth patch bytes").unwrap();

    // 1. Run "tofu pack" in the folder with NO arguments
    let mut pack_cmd = Command::cargo_bin("tofu").unwrap();
    pack_cmd
        .current_dir(&presets_folder)
        .arg("pack")
        .assert()
        .success()
        .stdout(predicate::str::contains("Archive created successfully!"))
        .stdout(predicate::str::contains("MyPresets.tofu"));

    let expected_archive = presets_folder.join("MyPresets.tofu");
    assert!(expected_archive.exists(), "MyPresets.tofu was not created!");

    // 2. Verify archive doesn't include itself
    let mut list_cmd = Command::cargo_bin("tofu").unwrap();
    list_cmd
        .current_dir(&presets_folder)
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("Pluck 01.fxp"))
        .stdout(predicate::str::contains("Bass 02.fxp"));

    let entries = tofu_core::list(&expected_archive).unwrap();
    assert_eq!(entries.len(), 2);
    assert!(!entries.iter().any(|e| e.path.contains("MyPresets.tofu")));

    // 3. Remove original presets so we can test clean unpack
    fs::remove_file(&file1).unwrap();
    fs::remove_file(&file2).unwrap();

    // 4. Run "tofu unpack --yes" in the folder with NO path specified
    let mut unpack_cmd = Command::cargo_bin("tofu").unwrap();
    unpack_cmd
        .current_dir(&presets_folder)
        .arg("unpack")
        .arg("--yes")
        .assert()
        .success()
        .stdout(predicate::str::contains("Lossless extraction verified!"))
        .stdout(predicate::str::contains("Removed archive 'MyPresets.tofu'"));

    // 5. Confirm original files are restored byte-for-byte identically
    assert_eq!(fs::read(&file1).unwrap(), b"Pluck synth patch bytes");
    assert_eq!(fs::read(&file2).unwrap(), b"Bass synth patch bytes");

    // 6. Confirm archive was safely removed
    assert!(!expected_archive.exists(), "Archive should have been safely deleted after unpack!");
}

#[test]
fn test_cli_unpack_keep_flag() {
    let dir = tempdir().unwrap();
    let src_file = dir.path().join("Lead.fxp");
    fs::write(&src_file, b"Lead preset content").unwrap();

    let archive = dir.path().join("LeadPack.tofu");

    // Pack
    let mut pack_cmd = Command::cargo_bin("tofu").unwrap();
    pack_cmd
        .arg("pack")
        .arg(&src_file)
        .arg(&archive)
        .assert()
        .success();

    assert!(archive.exists());

    let ext_dir = dir.path().join("ext");

    // Unpack with --keep
    let mut unpack_cmd = Command::cargo_bin("tofu").unwrap();
    unpack_cmd
        .arg("unpack")
        .arg(&archive)
        .arg(&ext_dir)
        .arg("--keep")
        .assert()
        .success()
        .stdout(predicate::str::contains("Kept (--keep flag specified)"));

    // Archive MUST still exist
    assert!(archive.exists(), "Archive must remain when --keep is used!");
    assert_eq!(fs::read(ext_dir.join("Lead.fxp")).unwrap(), b"Lead preset content");
}

#[test]
fn test_cli_unpack_never_deletes_corrupted_archive() {
    let dir = tempdir().unwrap();
    let src_file = dir.path().join("Synth.fxp");
    fs::write(&src_file, b"Good preset data").unwrap();

    let archive = dir.path().join("CorruptPack.tofu");

    // Pack
    let mut pack_cmd = Command::cargo_bin("tofu").unwrap();
    pack_cmd
        .arg("pack")
        .arg(&src_file)
        .arg(&archive)
        .assert()
        .success();

    // Corrupt a byte in the payload
    let mut bytes = fs::read(&archive).unwrap();
    bytes[85] ^= 0xAA;
    fs::write(&archive, &bytes).unwrap();

    let ext_dir = dir.path().join("ext_fail");

    // Unpack without --keep (default behavior would delete on success)
    let mut unpack_cmd = Command::cargo_bin("tofu").unwrap();
    unpack_cmd
        .arg("unpack")
        .arg(&archive)
        .arg(&ext_dir)
        .arg("--yes")
        .assert()
        .failure();

    // CRITICAL SAFETY REQUIREMENT: Corrupted archive MUST NEVER be deleted!
    assert!(archive.exists(), "CRITICAL: Corrupted archive was deleted! It must be preserved!");
}

#[test]
fn test_cli_unpack_no_archives_found_error() {
    let dir = tempdir().unwrap();

    // Run unpack in an empty directory
    let mut cmd = Command::cargo_bin("tofu").unwrap();
    cmd.current_dir(dir.path())
        .arg("unpack")
        .assert()
        .failure()
        .stderr(predicate::str::contains("No .tofu archive was found in the current directory"));
}

#[test]
fn test_cli_unpack_multiple_archives_error() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("preset.fxp");
    fs::write(&src, b"data").unwrap();

    // Create 2 archives in the directory
    tofu_core::pack(&src, dir.path().join("PackA.tofu"), &tofu_core::pack::PackOptions::default()).unwrap();
    tofu_core::pack(&src, dir.path().join("PackB.tofu"), &tofu_core::pack::PackOptions::default()).unwrap();

    // Run unpack without argument
    let mut cmd = Command::cargo_bin("tofu").unwrap();
    cmd.current_dir(dir.path())
        .arg("unpack")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Multiple TOFU archives were found"))
        .stderr(predicate::str::contains("PackA.tofu"))
        .stderr(predicate::str::contains("PackB.tofu"));
}

#[test]
fn test_cli_spaces_unicode_parentheses_paths() {
    let dir = tempdir().unwrap();
    let complex_dir = dir.path().join("Serum (Artist) [Pack #01] 日本語");
    fs::create_dir_all(&complex_dir).unwrap();

    let preset = complex_dir.join("Ultra Bass (Sub & Low) リード.fxp");
    let content = b"Preset data with special chars in path!";
    fs::write(&preset, content).unwrap();

    // Pack
    let mut pack_cmd = Command::cargo_bin("tofu").unwrap();
    pack_cmd
        .current_dir(&complex_dir)
        .arg("pack")
        .assert()
        .success();

    let archive = complex_dir.join("Serum (Artist) [Pack #01] 日本語.tofu");
    assert!(archive.exists());

    // Verify
    let mut verify_cmd = Command::cargo_bin("tofu").unwrap();
    verify_cmd
        .current_dir(&complex_dir)
        .arg("verify")
        .assert()
        .success()
        .stdout(predicate::str::contains("✓ TOFU archive is valid."));

    // Unpack with --keep
    let ext_dir = dir.path().join("restored_complex");
    let mut unpack_cmd = Command::cargo_bin("tofu").unwrap();
    unpack_cmd
        .arg("unpack")
        .arg(&archive)
        .arg(&ext_dir)
        .arg("--keep")
        .assert()
        .success();

    let restored = ext_dir.join("Ultra Bass (Sub & Low) リード.fxp");
    assert_eq!(fs::read(&restored).unwrap(), content);
}

#[test]
fn test_cli_verify_output_formatting() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("Kick.wav");
    fs::write(&src, b"RIFF....WAVEfmt ....data....").unwrap();

    let archive = dir.path().join("Kick.tofu");
    tofu_core::pack(&src, &archive, &tofu_core::pack::PackOptions::default()).unwrap();

    let mut cmd = Command::cargo_bin("tofu").unwrap();
    cmd.arg("verify")
        .arg(&archive)
        .assert()
        .success()
        .stdout(predicate::str::contains("✓ TOFU archive is valid."))
        .stdout(predicate::str::contains("✓ 1 files verified bit-for-bit lossless."))
        .stdout(predicate::str::contains("✓ Integrity checks passed."));
}
