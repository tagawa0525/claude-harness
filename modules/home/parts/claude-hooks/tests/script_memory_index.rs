//! `claude-hooks script memory-index MEMORY_DIR` が、メモリの索引（MEMORY.md）と実ファイル、本文の [[名前]] の参照先を
//! 突き合わせ、本文の崩れた日本語の語を辞書の検査（ja-terms check）の指摘から出すことを確かめる
//! （`~/.claude/skills/retro/scripts/memory-index.sh` が呼ぶ）。ja-terms は偽のコマンド（examples/fake_tool.rs）にする。
//! 本物の辞書の規則（コードスパンを数えないなど）は `modules/home/parts/tests/ja-terms.sh` が、sh から呼ぶ配備の確認は
//! `.claude/tests/scripts/retro.sh` が確かめる

mod common;

use common::{fake_tool, place_fake_tool};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Mem {
    root: PathBuf,
}

impl Drop for Mem {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct Run {
    code: i32,
    out: String,
    err: String,
}

impl Mem {
    fn new() -> Mem {
        assert!(
            fake_tool().is_file(),
            "{} が無い（cargo build --examples を先に流す）",
            fake_tool().display()
        );
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "memory-index-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("memory")).unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        place_fake_tool(&root.join(format!("bin/ja-terms{}", std::env::consts::EXE_SUFFIX)));
        Mem { root }
    }

    /// スクリプトに渡すメモリのディレクトリ（辞書の検査に渡すパスと、指摘の行の頭はこれに `/<ファイル>` を足したもの）
    fn dir(&self) -> String {
        self.root.join("memory").display().to_string()
    }

    fn write(&self, name: &str, text: &str) {
        std::fs::write(self.root.join("memory").join(name), text).unwrap();
    }

    fn remove(&self, name: &str) {
        std::fs::remove_file(self.root.join("memory").join(name)).unwrap();
    }

    fn ja_terms(&self) -> PathBuf {
        self.root
            .join(format!("bin/ja-terms{}", std::env::consts::EXE_SUFFIX))
    }

    fn log(&self) -> PathBuf {
        self.root.join("ja-terms.log")
    }

    /// 偽の ja-terms を、標準出力 out と終了コード exit で終えるものにして実行する
    fn run_with(&self, args: &[&str], out: &str, exit: i32) -> Run {
        self.run_ja_terms(args, &self.ja_terms(), out, exit)
    }

    fn run_ja_terms(&self, args: &[&str], ja_terms: &Path, out: &str, exit: i32) -> Run {
        let output = Command::new(env!("CARGO_BIN_EXE_claude-hooks"))
            .args(["script", "memory-index"])
            .args(args)
            .env("CLAUDE_HOOKS_JA_TERMS", ja_terms)
            .env("FAKE_JA_TERMS_OUT", out)
            .env("FAKE_JA_TERMS_EXIT", exit.to_string())
            .env("FAKE_JA_TERMS_LOG", self.log())
            .output()
            .unwrap();
        Run {
            code: output.status.code().unwrap(),
            out: String::from_utf8(output.stdout).unwrap(),
            err: String::from_utf8(output.stderr).unwrap(),
        }
    }

    /// 崩れた語の無い辞書の検査で実行する
    fn run(&self) -> Run {
        self.run_with(&[&self.dir()], "", 0)
    }
}

fn lines(out: &str) -> Vec<&str> {
    out.lines().collect()
}

/// 索引に外部と別のディレクトリへのリンクを持ち、項目の 1 つにファイルが無く、索引に無いファイルが 1 つあるメモリ
fn mismatched() -> Mem {
    let m = Mem::new();
    m.write(
        "MEMORY.md",
        "- [好み A](feedback-a.md) — 説明\n\
         - [事情 B](project-b.md) — 説明\n\
         - [消えた C](project-c.md) — 説明\n\
         - [外部](https://example.com/README.md) — 外部の文書、[ADR](../docs/adr/0001.md) を参照\n",
    );
    m.write(
        "feedback-a.md",
        "---\nname: a\n---\n\n本文 [[project-b|別名]] と [[project-b#節]]\n\n`[[ -n \"$x\" ]]`\n\n```bash\n[[ -z \"$y\" ]]\n```\n",
    );
    m.write(
        "project-b.md",
        "---\nname: b\n---\n\n本文 [[feedback-gone]] と [[feedback-a]]\n",
    );
    m.write("project-d.md", "---\nname: d\n---\n\n本文\n");
    m
}

/// 食い違いの無いメモリ（MEMORY.md、feedback-a.md、project-b.md）
fn consistent() -> Mem {
    let m = Mem::new();
    m.write(
        "MEMORY.md",
        "- [好み A](feedback-a.md) — 説明\n- [事情 B](project-b.md) — 説明\n",
    );
    m.write("feedback-a.md", "---\nname: a\n---\n\n本文\n");
    m.write("project-b.md", "---\nname: b\n---\n\n本文 [[feedback-a]]\n");
    m
}

#[test]
fn counts_the_index_entries_and_reports_missing_and_unindexed_files() {
    let m = mismatched();
    let r = m.run();
    assert_eq!(r.code, 0, "{}", r.err);
    let out = lines(&r.out);
    assert_eq!(out[0], "ENTRIES: 3");
    assert!(out.contains(&"MISSING: project-c.md"), "{out:?}");
    assert!(out.contains(&"UNINDEXED: project-d.md"), "{out:?}");
    assert!(!r.out.contains("MISSING: feedback-a.md"), "{out:?}");
    assert!(!r.out.contains("UNINDEXED: MEMORY.md"), "{out:?}");
}

#[test]
fn links_outside_the_directory_are_not_index_entries() {
    let r = mismatched().run();
    assert!(!r.out.contains("example.com"), "{}", r.out);
    assert!(!r.out.contains("../docs"), "{}", r.out);
}

#[test]
fn a_link_with_an_alias_or_a_section_resolves_by_its_name_and_code_is_not_read() {
    let r = mismatched().run();
    assert!(!r.out.contains("BROKEN_LINK: feedback-a.md"), "{}", r.out);
}

#[test]
fn a_link_to_a_name_without_a_file_is_broken() {
    let r = mismatched().run();
    let out = lines(&r.out);
    assert!(
        out.contains(&"BROKEN_LINK: project-b.md -> feedback-gone"),
        "{out:?}"
    );
    assert!(!r.out.contains("-> feedback-a"), "{out:?}");
    assert!(!r.out.contains("-> project-b"), "{out:?}");
}

#[test]
fn a_link_to_the_front_matter_name_of_a_file_is_not_broken() {
    let m = mismatched();
    m.write("project-d.md", "---\nname: d-name\n---\n\n本文\n");
    m.write("project-e.md", "---\nname: e\n---\n\n本文 [[d-name]]\n");
    let r = m.run();
    assert!(!r.out.contains("-> d-name"), "{}", r.out);
}

#[test]
fn the_quotes_of_the_name_are_removed_and_a_name_outside_front_matter_is_not_read() {
    let m = mismatched();
    m.write("project-d.md", "---\nname: \"d-quoted\"\n---\n\n本文\n");
    m.write("project-h.md", "---\nname: 'h-quoted'  \n---\n\n本文\n");
    m.write("project-f.md", "本文だけ\nname: bogus\n");
    m.write(
        "project-e.md",
        "---\nname: e\n---\n\n本文 [[d-quoted]] [[h-quoted]] [[bogus]]\n",
    );
    let r = m.run();
    assert!(!r.out.contains("-> d-quoted"), "{}", r.out);
    assert!(!r.out.contains("-> h-quoted"), "{}", r.out);
    assert!(
        lines(&r.out).contains(&"BROKEN_LINK: project-e.md -> bogus"),
        "{}",
        r.out
    );
}

#[test]
fn the_name_after_the_closing_line_of_front_matter_is_not_read() {
    let m = mismatched();
    m.write("project-d.md", "---\ndescription: x\n---\nname: late\n");
    m.write("project-e.md", "---\nname: e\n---\n\n本文 [[late]]\n");
    let r = m.run();
    assert!(
        lines(&r.out).contains(&"BROKEN_LINK: project-e.md -> late"),
        "{}",
        r.out
    );
}

#[test]
fn an_empty_link_name_is_skipped_and_tilde_fences_are_not_read() {
    let m = mismatched();
    m.write(
        "project-g.md",
        "---\nname: g\n---\n\n本文 [[#節]] [[ ]]\n\n~~~\n[[ -n x ]]\n~~~\n",
    );
    let r = m.run();
    assert_eq!(r.code, 0, "{}", r.err);
    assert!(!r.out.contains("BROKEN_LINK: project-g.md"), "{}", r.out);
}

#[test]
fn a_broken_link_is_reported_once_per_file_with_the_surrounding_spaces_trimmed() {
    let m = mismatched();
    m.write(
        "project-e.md",
        "---\nname: e\n---\n\n[[ gone ]] [[gone]] [[gone|別名]]\n",
    );
    let r = m.run();
    let broken: Vec<&str> = lines(&r.out)
        .into_iter()
        .filter(|l| l.starts_with("BROKEN_LINK: project-e.md"))
        .collect();
    assert_eq!(broken, ["BROKEN_LINK: project-e.md -> gone"]);
}

#[test]
fn index_links_starting_with_dot_slash_or_with_a_section_or_a_title_match_by_file_name() {
    let m = consistent();
    m.write(
        "MEMORY.md",
        "- [好み A](feedback-a.md \"題\") — 説明\n- [事情 B](./project-b.md#why) — 説明\n",
    );
    let r = m.run();
    assert_eq!(r.out, "ENTRIES: 2\n");
}

#[test]
fn a_consistent_memory_prints_only_the_count() {
    let r = consistent().run();
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(r.out, "ENTRIES: 2\n");
}

#[test]
fn all_md_files_including_the_index_go_to_one_term_check() {
    let m = consistent();
    m.write("notes.txt", "本文\n");
    m.run();
    let log = std::fs::read_to_string(m.log()).unwrap();
    let calls = lines(&log);
    assert_eq!(calls.len(), 1, "{calls:?}");
    let dir = m.dir();
    let mut args: Vec<&str> = calls[0]
        .strip_prefix("ja-terms check ")
        .unwrap()
        .split(' ')
        .collect();
    args.sort();
    let expected = [
        format!("{dir}/MEMORY.md"),
        format!("{dir}/feedback-a.md"),
        format!("{dir}/project-b.md"),
    ];
    assert_eq!(args, expected);
}

#[test]
fn the_term_check_findings_become_terms_lines_with_the_file_name_and_line() {
    let m = consistent();
    let dir = m.dir();
    let report = format!(
        "{dir}/project-b.md:6:3: 旧語 => 新語 [Error/prh]\n{dir}/feedback-a.md:12:1: 古い言い方 => 新しい言い方 [Error/prh]\n\n2 problems\n"
    );
    let r = m.run_with(&[&dir], &report, 1);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(
        lines(&r.out),
        [
            "ENTRIES: 2",
            "TERMS: project-b.md:6 旧語 => 新語",
            "TERMS: feedback-a.md:12 古い言い方 => 新しい言い方",
        ]
    );
}

// Windows のファイル名には : を使えない
#[cfg(unix)]
#[test]
fn a_file_name_with_a_colon_is_cut_at_the_known_file_name() {
    let m = consistent();
    m.write("a:1.md", "本文\n");
    m.write(
        "MEMORY.md",
        "- [A](feedback-a.md)\n- [B](project-b.md)\n- [C](a:1.md)\n",
    );
    let dir = m.dir();
    let report = format!("{dir}/a:1.md:2:5: 旧語 => 新語 [Error/prh]\n");
    let r = m.run_with(&[&dir], &report, 1);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(
        lines(&r.out),
        ["ENTRIES: 3", "TERMS: a:1.md:2 旧語 => 新語"]
    );
}

#[test]
fn findings_the_script_cannot_read_stop_it() {
    let m = consistent();
    let r = m.run_with(&[&m.dir()], "something else\n", 1);
    assert_eq!(r.code, 1);
    assert!(r.err.contains("ERROR:"), "{}", r.err);
    assert!(r.err.contains("something else"), "{}", r.err);
}

#[test]
fn a_failing_term_check_stops_it_after_the_other_lines() {
    let m = consistent();
    let r = m.run_with(&[&m.dir()], "check crashed\n", 2);
    assert_eq!(r.code, 1);
    assert_eq!(r.out, "ENTRIES: 2\n");
    assert!(r.err.contains("check crashed"), "{}", r.err);
    assert!(r.err.contains("ERROR:"), "{}", r.err);
    assert!(r.err.contains("終了コード 2"), "{}", r.err);
}

#[test]
fn a_missing_term_check_asks_for_the_install_after_the_other_lines() {
    let m = consistent();
    let missing = m.root.join("missing").join("ja-terms");
    let r = m.run_ja_terms(&[&m.dir()], &missing, "", 0);
    assert_eq!(r.code, 1);
    assert_eq!(r.out, "ENTRIES: 2\n");
    assert!(r.err.contains("ERROR:"), "{}", r.err);
    assert!(r.err.contains("rebuild"), "{}", r.err);
}

#[test]
fn a_missing_index_stops_it() {
    let m = Mem::new();
    let r = m.run();
    assert_eq!(r.code, 1);
    assert_eq!(r.out, "");
    assert!(
        r.err
            .contains(&format!("ERROR: 索引を読めません: {}/MEMORY.md", m.dir())),
        "{}",
        r.err
    );
}

#[test]
fn a_wrong_number_of_arguments_prints_the_usage() {
    let m = consistent();
    for args in [vec![], vec![m.dir(), m.dir()]] {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let r = m.run_with(&args, "", 0);
        assert_eq!(r.code, 1);
        assert_eq!(r.err, "Usage: memory-index.sh MEMORY_DIR\n");
    }
}

#[test]
fn a_missing_index_file_is_reported_for_each_entry_in_the_index() {
    let m = consistent();
    m.remove("project-b.md");
    let r = m.run();
    assert_eq!(r.out, "ENTRIES: 2\nMISSING: project-b.md\n");
}
