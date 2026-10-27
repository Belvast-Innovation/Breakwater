//! `breakwater snapshot --check` exit codes that do not need a build.
//!
//! The baseline is read before cargo is started, so a missing, malformed or
//! unsupported snapshot file fails with exit code 2 right away. The suite
//! directory below is a stub that is only inspected, never compiled.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir =
            std::env::temp_dir().join(format!("breakwater-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("suite/tests")).expect("scratch dir");
        std::fs::write(dir.join("suite/Cargo.toml"), "").expect("manifest stub");
        std::fs::write(dir.join("suite/tests/suite.rs"), "").expect("suite stub");
        Scratch(dir)
    }

    fn suite(&self) -> PathBuf {
        self.0.join("suite")
    }

    fn file(&self) -> PathBuf {
        self.0.join(".breakwater-snapshot.json")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn snapshot(args: &[&str], suite: &Path, file: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_breakwater"))
        .arg("--no-color")
        .arg("snapshot")
        .arg(suite)
        .args(args)
        .arg("--file")
        .arg(file)
        .output()
        .expect("runs the breakwater binary")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_missing_snapshot_is_a_configuration_error() {
    let s = Scratch::new("missing");
    let out = snapshot(&["--check"], &s.suite(), &s.file());
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("does not exist"), "{}", stderr(&out));
}

#[test]
fn a_malformed_snapshot_is_a_configuration_error() {
    let s = Scratch::new("malformed");
    for content in ["", "not json", "{}", "{\"format\": 1}"] {
        std::fs::write(s.file(), content).expect("writes");
        let out = snapshot(&["--check"], &s.suite(), &s.file());
        assert_eq!(out.status.code(), Some(2), "{content:?}: {}", stderr(&out));
        assert!(
            stderr(&out).contains("not a valid snapshot"),
            "{}",
            stderr(&out)
        );
    }
}

#[test]
fn an_unsupported_format_is_a_configuration_error() {
    let s = Scratch::new("format");
    std::fs::write(s.file(), "{\"format\": 99}").expect("writes");
    let out = snapshot(&["--check"], &s.suite(), &s.file());
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("unsupported snapshot format 99"));
}

#[test]
fn recording_does_not_overwrite_a_file_it_cannot_read() {
    let s = Scratch::new("record");
    std::fs::write(s.file(), "precious but broken").expect("writes");
    let out = snapshot(&[], &s.suite(), &s.file());
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert_eq!(
        std::fs::read_to_string(s.file()).expect("reads"),
        "precious but broken"
    );
}

#[test]
fn a_directory_that_is_not_a_suite_is_a_configuration_error() {
    let s = Scratch::new("notasuite");
    let empty = s.0.join("empty");
    std::fs::create_dir_all(&empty).expect("dir");
    let out = snapshot(&["--check"], &empty, &s.file());
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("no Cargo.toml"), "{}", stderr(&out));
}

#[test]
fn check_cannot_be_combined_with_run_parameters() {
    let s = Scratch::new("conflict");
    let out = snapshot(&["--check", "--seed", "7"], &s.suite(), &s.file());
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    let out = snapshot(&["--tolerance", "5%"], &s.suite(), &s.file());
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    let out = snapshot(&["--check", "--tolerance", "lots"], &s.suite(), &s.file());
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
}
