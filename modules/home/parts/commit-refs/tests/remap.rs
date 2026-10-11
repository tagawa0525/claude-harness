//! `commit-refs remap` を一時的な git リポジトリで走らせ、作業ツリーの文書の中のコミットの番号を、対応表で
//! 付け直すことを確かめる。

mod common;

use std::fs;
use std::process::{Command, Stdio};

use common::*;

#[test]
fn rebase_replaces_old_hashes_with_new_ones_of_the_same_length() {
    let t = TempRepo::new("rebase");
    t.git(&["switch", "-q", "-c", "feat"]);
    let old = t.commit_file("src.txt", "feat: measured");
    t.record(
        "notes.md",
        &format!("measured at {} and {}; full {old}", &old[..8], &old[..12]),
    );
    t.git(&["switch", "-q", "main"]);
    t.commit_file("other.txt", "chore: main moves");
    t.git(&["switch", "-q", "feat"]);
    t.git(&["rebase", "-q", "main"]);
    let new = t.rev("HEAD~1");

    let out = t.remap_in("", &[], &t.rewritten());
    assert_ok(&out);
    assert_eq!(
        t.read("notes.md"),
        format!("measured at {} and {}; full {new}\n", &new[..8], &new[..12])
    );
    let text = stdout(&out);
    assert!(text.contains("REPLACED: notes.md 3\n"), "{text}");
    assert!(text.contains("FILES: 1\n"), "{text}");
    assert!(text.contains("REFS: 3\n"), "{text}");
}

#[test]
fn replacements_are_left_uncommitted_in_the_working_tree() {
    let t = TempRepo::new("uncommitted");
    let old = t.commit_file("src.txt", "feat: measured");
    t.record("notes.md", &format!("at {}", &old[..8]));
    let map = t.map_file(&format!("{old} {NEW4}\n"));
    assert_ok(&t.remap(&[&map]));
    assert_eq!(t.git(&["status", "--porcelain"]), " M notes.md\n");
}

#[test]
fn amend_replaces_the_hash_before_the_fix_with_the_one_after() {
    let t = TempRepo::new("amend");
    let old = t.commit_file("src.txt", "feat: measured");
    t.write("README.md", &format!("result from {}\n", &old[..7]));
    t.write("src.txt", "feat: measured\nfix\n");
    t.git(&["add", "src.txt"]);
    t.git(&["commit", "-q", "--amend", "--no-edit"]);
    let new = t.head();

    let out = t.remap_in("", &[], &t.rewritten());
    assert_ok(&out);
    assert_eq!(t.read("README.md"), format!("result from {}\n", &new[..7]));
}

#[test]
fn reads_the_map_from_a_file_argument() {
    let t = TempRepo::new("arg");
    let old = t.commit_file("src.txt", "feat: measured");
    t.record("notes.md", &format!("at {}", &old[..8]));
    let map = t.map_file(&format!("{old} 1111111111111111111111111111111111111111\n"));
    assert_ok(&t.remap(&[&map]));
    assert_eq!(t.read("notes.md"), "at 11111111\n");
}

#[test]
fn commit_map_skips_the_header_and_keeps_unchanged_commits() {
    let t = TempRepo::new("commit-map");
    let a = t.commit_file("a.txt", "feat: a");
    let b = t.commit_file("b.txt", "feat: b");
    t.record("notes.md", &format!("a {}, b {}", &a[..8], &b[..8]));
    let map = t.map_file(&format!(
        "old                                      new\n{a} 2222222222222222222222222222222222222222\n{b} {b}\n"
    ));
    let out = t.remap(&[&map]);
    assert_ok(&out);
    assert_eq!(t.read("notes.md"), format!("a 22222222, b {}\n", &b[..8]));
    assert!(stdout(&out).contains("REFS: 1\n"), "{}", stdout(&out));
}

#[test]
fn a_reference_to_a_pruned_commit_stops_without_changes() {
    let t = TempRepo::new("pruned");
    let a = t.commit_file("a.txt", "feat: a");
    let b = t.commit_file("b.txt", "feat: b");
    let line = format!("a {}, b {}", &a[..8], &b[..8]);
    t.record("notes.md", &line);
    let map = t.map_file(&format!(
        "old                                      new\n{a} {ZERO}\n{b} 3333333333333333333333333333333333333333\n"
    ));
    let out = t.remap(&[&map]);
    assert_error(&out);
    assert!(stderr(&out).contains(&a[..8]), "{}", stderr(&out));
    assert_eq!(t.read("notes.md"), format!("{line}\n"));
}

#[test]
fn an_ambiguous_short_hash_stops_without_changing_any_file() {
    let t = TempRepo::new("ambiguous");
    t.record("ok.md", "unique abcdef0123");
    t.record("notes.md", "short abcdef0");
    let map = t.map_file(&format!("{OLD_A} {NEW4}\n{OLD_B} {NEW5}\n"));
    let out = t.remap(&[&map]);
    assert_error(&out);
    assert!(stderr(&out).contains("abcdef0"), "{}", stderr(&out));
    assert_eq!(t.read("notes.md"), "short abcdef0\n");
    assert_eq!(t.read("ok.md"), "unique abcdef0123\n");
}

#[test]
fn an_old_hash_with_two_different_new_hashes_stops() {
    let t = TempRepo::new("conflict");
    let map = t.map_file(&format!("{OLD_A} {NEW4}\n{OLD_A} {NEW5}\n"));
    assert_error(&t.remap(&[&map]));
}

#[test]
fn an_old_hash_also_listed_as_unchanged_or_pruned_with_another_new_hash_stops() {
    let t = TempRepo::new("conflict2");
    for first in [OLD_A, ZERO] {
        let map = t.map_file(&format!("{OLD_A} {first}\n{OLD_A} {NEW5}\n"));
        assert_error(&t.remap(&[&map]));
    }
}

#[test]
fn a_line_without_full_hashes_stops() {
    let t = TempRepo::new("bad-map");
    let map = t.map_file("abcdef0 4444444\n");
    assert_error(&t.remap(&[&map]));
}

#[test]
fn leaves_untracked_files_unmapped_hex_word_parts_and_short_hex_alone() {
    let t = TempRepo::new("untouched");
    let old = t.commit_file("src.txt", "feat: measured");
    t.record("notes.md", &format!("at {}", &old[..8]));
    let short = &old[..8];
    let line = format!(
        "x{short} {short}g {} deadbeef {short}_1 cafebabe12",
        &old[..6]
    );
    t.record("other.md", &line);
    t.write("untracked.md", &format!("untracked {short}\n"));
    let map = t.map_file(&format!("{old} 6666666666666666666666666666666666666666\n"));
    let out = t.remap(&[&map]);
    assert_ok(&out);
    assert_eq!(t.read("notes.md"), "at 66666666\n");
    assert_eq!(t.read("other.md"), format!("{line}\n"));
    assert_eq!(t.read("untracked.md"), format!("untracked {short}\n"));
    assert!(!stdout(&out).contains("other.md"), "{}", stdout(&out));
}

#[test]
fn replaces_in_files_whose_names_are_not_utf8() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let t = TempRepo::new("non-utf8");
    let old = t.commit_file("src.txt", "feat: measured");
    let name = OsStr::from_bytes(b"n\xff.md");
    fs::write(t.dir.join(name), format!("at {}\n", &old[..8])).unwrap();
    t.commit_all("docs: weird name");
    let map = t.map_file(&format!("{old} 6666666666666666666666666666666666666666\n"));
    assert_ok(&t.remap(&[&map]));
    assert_eq!(fs::read(t.dir.join(name)).unwrap(), b"at 66666666\n");
}

#[test]
fn leaves_binary_files_alone() {
    let t = TempRepo::new("binary");
    let old = t.commit_file("src.txt", "feat: measured");
    let data = format!("bin\0{}\n", &old[..8]);
    t.write("blob.bin", &data);
    t.commit_all("chore: binary");
    let map = t.map_file(&format!("{old} 6666666666666666666666666666666666666666\n"));
    assert_ok(&t.remap(&[&map]));
    assert_eq!(t.read("blob.bin"), data);
}

#[test]
fn does_not_write_through_tracked_symlinks() {
    let t = TempRepo::new("symlink");
    let old = t.commit_file("src.txt", "feat: measured");
    t.record("target.md", &format!("at {}", &old[..8]));
    std::os::unix::fs::symlink("target.md", t.path("link.md")).unwrap();
    t.commit_all("docs: link");
    let map = t.map_file(&format!("{old} 6666666666666666666666666666666666666666\n"));
    let out = t.remap(&[&map]);
    assert_ok(&out);
    assert_eq!(t.read("target.md"), "at 66666666\n");
    assert!(
        fs::symlink_metadata(t.path("link.md"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(stdout(&out).contains("REFS: 1\n"), "{}", stdout(&out));
}

#[test]
fn reports_zero_when_there_is_nothing_to_replace() {
    let t = TempRepo::new("none");
    let map = t.map_file(&format!("{OLD_A} {NEW4}\n"));
    let out = t.remap(&[&map]);
    assert_ok(&out);
    assert!(stdout(&out).contains("FILES: 0\n"), "{}", stdout(&out));
    assert!(stdout(&out).contains("REFS: 0\n"), "{}", stdout(&out));
}

#[test]
fn covers_the_whole_working_tree_when_called_from_a_subdirectory() {
    let t = TempRepo::new("subdir");
    let old = t.commit_file("src.txt", "feat: measured");
    t.record("docs/issues/x.md", &format!("at {}", &old[..8]));
    let map = t.map_file(&format!("{old} 7777777777777777777777777777777777777777\n"));
    let out = t.remap_in("docs", &[&map], "");
    assert_ok(&out);
    assert_eq!(t.read("docs/issues/x.md"), "at 77777777\n");
    assert!(
        stdout(&out).contains("REPLACED: docs/issues/x.md 1\n"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn more_than_one_map_argument_is_a_usage_error() {
    let t = TempRepo::new("usage");
    let out = t.remap(&["a", "b"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
    assert!(stderr(&out).contains("Usage:"), "{}", stderr(&out));
}

#[test]
fn words_longer_than_64_hex_digits_and_uppercase_hex_are_not_hashes() {
    let t = TempRepo::new("not-hashes");
    let old = t.commit_file("src.txt", "feat: measured");
    let long = format!("{}{}", &old[..8], "0".repeat(57));
    let upper = old[..8].to_uppercase();
    let line = format!("{long} {upper}");
    t.record("notes.md", &line);
    let map = t.map_file(&format!("{old} {NEW4}\n"));
    let out = t.remap(&[&map]);
    assert_ok(&out);
    assert_eq!(t.read("notes.md"), format!("{line}\n"));
}

#[test]
fn non_ascii_characters_are_word_boundaries() {
    let t = TempRepo::new("non-ascii");
    let old = t.commit_file("src.txt", "feat: measured");
    t.record("notes.md", &format!("測定{}の結果", &old[..8]));
    let map = t.map_file(&format!("{old} {NEW4}\n"));
    assert_ok(&t.remap(&[&map]));
    assert_eq!(t.read("notes.md"), "測定44444444の結果\n");
}

#[test]
fn reports_every_unreadable_line_of_the_map() {
    let t = TempRepo::new("bad-lines");
    let map = t.map_file("x y\nabc\n");
    let out = t.remap(&[&map]);
    assert_error(&out);
    assert!(stderr(&out).contains("1 行目"), "{}", stderr(&out));
    assert!(stderr(&out).contains("2 行目"), "{}", stderr(&out));
}

#[test]
fn a_line_whose_old_and_new_hashes_differ_in_length_stops() {
    let t = TempRepo::new("length");
    let old = t.commit_file("src.txt", "feat: measured");
    t.record("notes.md", &format!("at {}", &old[..8]));
    let map = t.map_file(&format!("{old} {}\n", "6".repeat(64)));
    let out = t.remap(&[&map]);
    assert_error(&out);
    assert_eq!(t.read("notes.md"), format!("at {}\n", &old[..8]));
}

#[test]
fn a_write_failure_keeps_the_file_intact_and_names_the_files_already_rewritten() {
    use std::os::unix::fs::PermissionsExt;

    let t = TempRepo::new("write-failure");
    let old = t.commit_file("src.txt", "feat: measured");
    let line = format!("at {}", &old[..8]);
    t.record("a.md", &line);
    t.record("locked/b.md", &line);
    // ディレクトリに書けないと、置き換えた内容を置く一時ファイルを作れない
    fs::set_permissions(t.path("locked"), fs::Permissions::from_mode(0o555)).unwrap();
    let map = t.map_file(&format!("{old} {NEW4}\n"));
    let out = t.remap(&[&map]);
    fs::set_permissions(t.path("locked"), fs::Permissions::from_mode(0o755)).unwrap();
    assert_error(&out);
    assert_eq!(t.read("a.md"), "at 44444444\n");
    assert_eq!(t.read("locked/b.md"), format!("{line}\n"));
    let err = stderr(&out);
    assert!(err.contains("locked/b.md"), "{err}");
    assert!(err.contains("書き換えたファイル: a.md"), "{err}");
    assert!(!err.contains("何も書き換えていない"), "{err}");
}

#[test]
fn keeps_the_permissions_of_rewritten_files() {
    use std::os::unix::fs::PermissionsExt;

    let t = TempRepo::new("permissions");
    let old = t.commit_file("src.txt", "feat: measured");
    t.write("run.sh", &format!("# at {}\n", &old[..8]));
    fs::set_permissions(t.path("run.sh"), fs::Permissions::from_mode(0o750)).unwrap();
    t.commit_all("chore: script");
    let map = t.map_file(&format!("{old} {NEW4}\n"));
    assert_ok(&t.remap(&[&map]));
    assert_eq!(t.read("run.sh"), "# at 44444444\n");
    let mode = fs::metadata(t.path("run.sh")).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o750);
}

#[test]
fn works_in_a_repository_whose_directory_name_ends_with_a_space() {
    let t = TempRepo::new("outer");
    let inner = t.path("inner ");
    fs::create_dir(&inner).unwrap();
    let git = |args: &[&str]| {
        let mut command = Command::new("git");
        command
            .current_dir(&inner)
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args);
        isolate(&mut command);
        let output = command.output().unwrap();
        assert!(output.status.success(), "git {args:?}");
        String::from_utf8(output.stdout).unwrap()
    };
    git(&["init", "-q", "-b", "main"]);
    fs::write(inner.join("src.txt"), "x\n").unwrap();
    git(&["add", "src.txt"]);
    git(&["commit", "-q", "-m", "feat: measured"]);
    let old = git(&["rev-parse", "HEAD"]).trim().to_string();
    fs::write(inner.join("notes.md"), format!("at {}\n", &old[..8])).unwrap();
    git(&["add", "notes.md"]);
    git(&["commit", "-q", "-m", "docs: record"]);
    let map = t.map_file(&format!("{old} {NEW4}\n"));
    let out = run(&inner, &["remap", &map], "");
    assert_ok(&out);
    assert_eq!(
        fs::read_to_string(inner.join("notes.md")).unwrap(),
        "at 44444444\n"
    );
}

#[test]
fn rewrites_every_file_even_when_stdout_cannot_be_written() {
    let t = TempRepo::new("stdout-full");
    let old = t.commit_file("src.txt", "feat: measured");
    t.record("a.md", &format!("at {}", &old[..8]));
    t.record("b.md", &format!("at {}", &old[..8]));
    let map = t.map_file(&format!("{old} {NEW4}\n"));
    let mut command = Command::new(env!("CARGO_BIN_EXE_commit-refs"));
    command
        .current_dir(&t.dir)
        .args(["remap", &map])
        .stdout(fs::File::create("/dev/full").unwrap())
        .stderr(Stdio::piped());
    isolate(&mut command);
    let out = command.output().unwrap();
    assert_error(&out);
    assert_eq!(t.read("a.md"), "at 44444444\n");
    assert_eq!(t.read("b.md"), "at 44444444\n");
}

#[test]
fn accepts_a_map_path_that_is_not_utf8() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let t = TempRepo::new("map-non-utf8");
    let old = t.commit_file("src.txt", "feat: measured");
    t.record("notes.md", &format!("at {}", &old[..8]));
    let map = t.path(".git").join(OsStr::from_bytes(b"m\xff.map"));
    fs::write(&map, format!("{old} {NEW4}\n")).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_commit-refs"));
    command
        .current_dir(&t.dir)
        .arg("remap")
        .arg(&map)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    isolate(&mut command);
    let out = command.output().unwrap();
    assert_ok(&out);
    assert_eq!(t.read("notes.md"), "at 44444444\n");
}

#[test]
fn a_zero_marker_of_any_length_marks_a_pruned_commit() {
    let t = TempRepo::new("zero-marker");
    let old = "abcdef01".repeat(8);
    t.record("notes.md", &format!("at {}", &old[..8]));
    let map = t.map_file(&format!("{old} {ZERO}\n"));
    let out = t.remap(&[&map]);
    assert_error(&out);
    assert!(stderr(&out).contains("消えたコミット"), "{}", stderr(&out));
}

#[test]
fn an_unreadable_file_does_not_hide_other_reasons_to_stop() {
    use std::os::unix::fs::PermissionsExt;

    let t = TempRepo::new("unreadable");
    t.record("a.md", "short abcdef0");
    t.record("b.md", "short abcdef0");
    fs::set_permissions(t.path("b.md"), fs::Permissions::from_mode(0o000)).unwrap();
    let map = t.map_file(&format!("{OLD_A} {NEW4}\n{OLD_B} {NEW5}\n"));
    let out = t.remap(&[&map]);
    fs::set_permissions(t.path("b.md"), fs::Permissions::from_mode(0o644)).unwrap();
    assert_error(&out);
    let err = stderr(&out);
    assert!(err.contains("a.md: abcdef0"), "{err}");
    assert!(err.contains("b.md"), "{err}");
}

#[test]
fn every_stop_before_writing_says_nothing_was_rewritten() {
    let t = TempRepo::new("nothing");
    let map = t.map_file("abc\n");
    let out = t.remap(&[&map]);
    assert_error(&out);
    assert!(
        stderr(&out).contains("何も書き換えていない"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn rewrites_files_whose_names_are_near_the_length_limit() {
    let t = TempRepo::new("long-name");
    let old = t.commit_file("src.txt", "feat: measured");
    // Linux のファイル名の上限（NAME_MAX）は 255 バイト
    let name = format!("{}.md", "n".repeat(240));
    t.record(&name, &format!("at {}", &old[..8]));
    let map = t.map_file(&format!("{old} {NEW4}\n"));
    assert_ok(&t.remap(&[&map]));
    assert_eq!(t.read(&name), "at 44444444\n");
}

#[test]
fn a_short_hash_matching_a_changed_and_an_unchanged_commit_stops_as_ambiguous() {
    let t = TempRepo::new("ambiguous-unchanged");
    t.record("notes.md", "short abcdef0");
    let map = t.map_file(&format!("{OLD_A} {NEW4}\n{OLD_B} {OLD_B}\n"));
    let out = t.remap(&[&map]);
    assert_error(&out);
    assert!(stderr(&out).contains("曖昧"), "{}", stderr(&out));
    assert_eq!(t.read("notes.md"), "short abcdef0\n");
}

#[test]
fn a_reference_to_an_unchanged_commit_is_kept() {
    let t = TempRepo::new("unchanged");
    t.record("notes.md", "at abcdef0f and abcdef01");
    let map = t.map_file(&format!("{OLD_A} {NEW4}\n{OLD_B} {OLD_B}\n"));
    let out = t.remap(&[&map]);
    assert_ok(&out);
    assert_eq!(t.read("notes.md"), "at abcdef0f and 44444444\n");
}

#[test]
fn a_map_whose_new_hash_is_also_an_old_hash_stops() {
    let t = TempRepo::new("chain");
    t.record("notes.md", "at abcdef01");
    // A → B と B → C の連鎖は、1 回の置き換えでは A が C にならず、呼び直すと書き換え済みの B が C になる
    let map = t.map_file(&format!("{OLD_A} {OLD_B}\n{OLD_B} {NEW5}\n"));
    let out = t.remap(&[&map]);
    assert_error(&out);
    assert!(stderr(&out).contains("連鎖"), "{}", stderr(&out));
    assert_eq!(t.read("notes.md"), "at abcdef01\n");
}

#[test]
fn counts_a_reference_whose_new_hash_shares_the_written_prefix() {
    // 書いた桁の範囲で新と旧が同じでも、変わったコミットを指す語として数える
    let t = TempRepo::new("same-prefix");
    t.record("notes.md", "short abcdef0");
    let new = format!("abcdef0{}", "9".repeat(33));
    let map = t.map_file(&format!("{OLD_A} {new}\n"));
    let out = t.remap(&[&map]);
    assert_ok(&out);
    let text = stdout(&out);
    assert!(text.contains("REPLACED: notes.md 1\n"), "{text}");
    assert!(text.contains("REFS: 1\n"), "{text}");
}
