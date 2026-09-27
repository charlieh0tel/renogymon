//! The `check` command mails once when the archive goes stale and once when it
//! recovers, via a fake sendmail that records each message.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

fn run_check(dest: &Path, sendmail: &Path, state: &Path) -> bool {
    Command::new(env!("CARGO_BIN_EXE_renogymon-archiver-puller"))
        .args(["--dest", dest.to_str().unwrap(), "check"])
        .args(["--sendmail", sendmail.to_str().unwrap()])
        .args(["--state-file", state.to_str().unwrap()])
        .env_remove("CHECK_VM_URL")
        .status()
        .unwrap()
        .success()
}

#[test]
fn check_mails_on_change_only() {
    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("archive");
    let mail = tmp.path().join("mail.txt");
    let state = tmp.path().join("state");
    let sendmail = tmp.path().join("sendmail");
    fs::create_dir_all(&dest).unwrap();
    fs::write(&sendmail, format!("#!/bin/sh\ncat >> {}\n", mail.display())).unwrap();
    fs::set_permissions(&sendmail, fs::Permissions::from_mode(0o755)).unwrap();

    // Empty archive: skipped, healthy, no mail.
    assert!(run_check(&dest, &sendmail, &state));
    assert!(!mail.exists());

    // Stale archive: one alert, repeated runs stay quiet.
    fs::write(dest.join("renogy_2000-01-01.parquet"), b"x").unwrap();
    assert!(!run_check(&dest, &sendmail, &state));
    assert!(!run_check(&dest, &sendmail, &state));
    let text = fs::read_to_string(&mail).unwrap();
    assert_eq!(
        text.matches("Subject: [renogymon] ALERT: archive stale")
            .count(),
        1
    );

    // Fresh archive: one recovery message.
    let today = chrono::Utc::now().date_naive();
    fs::write(dest.join(format!("renogy_{today}.parquet")), b"x").unwrap();
    assert!(run_check(&dest, &sendmail, &state));
    let text = fs::read_to_string(&mail).unwrap();
    assert!(text.contains("Subject: [renogymon] OK"), "{text}");
    assert!(text.contains("RESOLVED archive"), "{text}");
}
