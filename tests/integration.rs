use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

fn scaffold(dir: &std::path::Path, files: &[(&str, &str)]) {
    for (name, content) in files {
        let path = dir.join(name);
        std::fs::write(path, content).expect("write test file");
    }
}

fn run_one_shot(dir: &std::path::Path, patterns: &[&str], lines: usize) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_folor"))
        .arg("-C")
        .arg(dir)
        .arg("-n")
        .arg(lines.to_string())
        .args(patterns)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("folor binary not found")
}

#[test]
fn last_n_lines_of_matching_files() {
    let dir = tempfile::tempdir().unwrap();
    scaffold(
        dir.path(),
        &[("app.log", "a1\na2\na3\n"), ("db.log", "b1\nb2\nb3\n")],
    );

    let out = run_one_shot(dir.path(), &["*.log"], 2);
    let _stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    let combined = format!("{}{}", _stdout, stderr);
    assert!(combined.contains("a2"));
    assert!(combined.contains("a3"));
    assert!(combined.contains("b2"));
    assert!(combined.contains("b3"));
}

#[test]
fn no_patterns_falls_back_to_stdin() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_folor"))
        .arg("-n")
        .arg("2")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    {
        let stdin = child.stdin.as_mut().unwrap();
        stdin.write_all(b"line1\nline2\nline3\n").unwrap();
    }

    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("line2"));
    assert!(stdout.contains("line3"));
}

#[test]
fn binary_file_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("data.bin"), [0u8, 1, 2, 3]).unwrap();
    std::fs::write(dir.path().join("text.log"), b"hello\nworld\n").unwrap();

    let out = run_one_shot(dir.path(), &["*.*"], 10);
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("skipping"));
}

#[test]
fn lines_zero_exits_without_output() {
    let dir = tempfile::tempdir().unwrap();
    scaffold(dir.path(), &[("app.log", "a\nb\nc\n")]);

    let out = run_one_shot(dir.path(), &["*.log"], 0);
    assert!(out.status.success());
}

#[test]
fn older_than_filters_old_files() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("old.log");
    std::fs::write(&old, b"old\n").unwrap();

    let two_hours = filetime::FileTime::from_system_time(
        std::time::SystemTime::now() - Duration::from_secs(7201),
    );
    filetime::set_file_mtime(&old, two_hours).unwrap();
    std::fs::write(dir.path().join("fresh.log"), b"fresh\n").unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_folor"))
        .arg("-C")
        .arg(dir.path())
        .arg("-n")
        .arg("5")
        .arg("--older-than")
        .arg("1h")
        .arg("*.log")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();

    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("fresh"));
    assert!(!stdout.contains("old"));
}

#[test]
fn invalid_glob_exits_with_error() {
    let dir = tempfile::tempdir().unwrap();
    let out = run_one_shot(dir.path(), &["["], 10);
    assert!(!out.status.success());
}

#[test]
fn no_files_matched_is_success() {
    let dir = tempfile::tempdir().unwrap();
    let out = run_one_shot(dir.path(), &["*.log"], 10);
    assert!(out.status.success());
}

#[test]
fn version_and_help_flags() {
    let ver = Command::new(env!("CARGO_BIN_EXE_folor"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(ver.status.success());
    assert!(String::from_utf8(ver.stdout).unwrap().contains("folor"));

    let help = Command::new(env!("CARGO_BIN_EXE_folor"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(help.status.success());
    let h = String::from_utf8(help.stdout).unwrap();
    assert!(h.contains("--tail"));
    assert!(h.contains("--older-than"));
}

#[test]
fn follow_mode_picks_up_new_lines() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("app.log"), b"initial\n").unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_folor"))
        .arg("--tail")
        .arg("-n")
        .arg("0")
        .arg("-C")
        .arg(dir.path())
        .arg("*.log")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    std::thread::sleep(Duration::from_millis(800));

    {
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join("app.log"))
            .unwrap();
        writeln!(f, "new line").unwrap();
    }

    std::thread::sleep(Duration::from_millis(300));

    child.kill().unwrap();
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("new line"));
}

#[test]
fn pipe_mode_suppresses_filenames() {
    let dir = tempfile::tempdir().unwrap();
    scaffold(dir.path(), &[("a.log", "hello\n"), ("b.log", "world\n")]);

    let out = run_one_shot(dir.path(), &["*.log"], 1);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(!stdout.contains(".log:"));
}

#[test]
fn filename_flag_forces_prefixes() {
    let dir = tempfile::tempdir().unwrap();
    scaffold(dir.path(), &[("a.log", "hello\n"), ("b.log", "world\n")]);

    let out = Command::new(env!("CARGO_BIN_EXE_folor"))
        .arg("-C")
        .arg(dir.path())
        .arg("-n")
        .arg("1")
        .arg("--filename")
        .arg("*.log")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();

    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains(".log:"));
}
