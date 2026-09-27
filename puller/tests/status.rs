//! The puller `status` gap audit reports calendar days missing from the archive dir.

use std::fs;
use std::process::Command;

#[test]
fn status_reports_missing_days() {
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("archive");
    fs::create_dir_all(&dest).unwrap();
    // 06-02 is a single missing day; 06-04 .. 06-06 is a missing range.
    for name in [
        "renogy_2026-06-01.parquet",
        "renogy_2026-06-03.parquet",
        "renogy_2026-06-07.parquet",
    ] {
        fs::write(dest.join(name), b"x").unwrap();
    }

    let out = Command::new(env!("CARGO_BIN_EXE_renogymon-archiver-puller"))
        .args(["--dest", dest.to_str().unwrap(), "status"])
        .output()
        .unwrap();

    assert!(out.status.success(), "status exited with {}", out.status);
    let stdout = String::from_utf8_lossy(&out.stdout);
    for expected in [
        "4 missing day(s) in 2 range(s)",
        "  2026-06-02\n",
        "  2026-06-04 .. 2026-06-06 (3 days)",
    ] {
        assert!(
            stdout.contains(expected),
            "{expected:?} not reported; stdout was:\n{stdout}"
        );
    }
}
