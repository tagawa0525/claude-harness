//! 共通の辞書を、ビルドに埋めず、`JA_TERMS_COMMON_DICTIONARY` のパスか、無ければホームの下の
//! `.config/ja-terms/prh.yml`（`claude-setup sync` が置く場所）から読むこと。辞書が無いか読めなければ、検査できなかった
//! として 2 で終わること（ADR-0097）。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

const VARIABLE: &str = "JA_TERMS_COMMON_DICTIONARY";

/// ビルドした ja-terms が知らない規則（ふがふが => ほげ）だけの辞書
const RULES: &str = "version: 1\nrules:\n  - expected: ほげ\n    pattern: ふがふが\n";

/// 規則の語を含む、Conventional Commits の件名のメッセージ
const MESSAGE: &str = "docs: x\n\nふがふがの値\n";

/// テストごとの空のディレクトリ（作業ツリーの外）
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Scratch {
    fn new(name: &str) -> Scratch {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "ja-terms-common-{name}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn write(&self, path: &str, text: &str) -> PathBuf {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }

    /// dir の中で git のリポジトリを作り、doc.md をステージする（staged の検査の対象）
    fn staged_repo(&self) -> PathBuf {
        let repo = self.0.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join("doc.md"), "# 題\n\nふがふがの値\n").unwrap();
        for args in [["init", "-q"], ["add", "doc.md"]] {
            let out = git(&repo, &self.0).args(args).output().unwrap();
            assert!(out.status.success(), "{}", stderr(&out));
        }
        repo
    }
}

/// 利用者の git の設定と、外の git の環境変数に左右されない git
fn git(dir: &Path, base: &Path) -> Command {
    let mut command = Command::new("git");
    isolate(&mut command, base);
    command.current_dir(dir);
    command
}

fn isolate(command: &mut Command, base: &Path) {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
    command
        .env("GIT_CONFIG_GLOBAL", base.join("gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1");
}

/// dir で、ホームを home にして ja-terms を実行する。dictionary が Some なら環境変数でその辞書を渡し、None なら
/// 環境変数を外す（ホームの下の辞書を読む）
fn ja_terms(
    scratch: &Scratch,
    dir: &Path,
    home: &Path,
    dictionary: Option<&Path>,
    args: &[&str],
    stdin: &str,
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ja-terms"));
    isolate(&mut command, &scratch.0);
    // std::env::home_dir は Unix で HOME を、Windows で USERPROFILE を見る
    command
        .current_dir(dir)
        .env("HOME", home)
        .env("USERPROFILE", home);
    match dictionary {
        Some(path) => command.env(VARIABLE, path),
        None => command.env_remove(VARIABLE),
    };
    let mut child = command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// 4 つのサブコマンドを、それぞれの対象（ファイル、標準入力、メッセージのファイル、ステージした中身）に規則の語を
/// 置いて実行した結果（名前と出力）
fn every_subcommand(
    scratch: &Scratch,
    home: &Path,
    dictionary: Option<&Path>,
) -> Vec<(&'static str, Output)> {
    scratch.write("doc.md", "# 題\n\nふがふがの値\n");
    let message = scratch.write("COMMIT_EDITMSG", MESSAGE);
    let repo = scratch.staged_repo();
    let dir = scratch.0.as_path();
    vec![
        (
            "check",
            ja_terms(scratch, dir, home, dictionary, &["check", "doc.md"], ""),
        ),
        (
            "message",
            ja_terms(scratch, dir, home, dictionary, &["message"], MESSAGE),
        ),
        (
            "commit-msg",
            ja_terms(
                scratch,
                dir,
                home,
                dictionary,
                &["commit-msg", message.to_str().unwrap()],
                "",
            ),
        ),
        (
            "staged",
            ja_terms(scratch, &repo, home, dictionary, &["staged"], ""),
        ),
    ]
}

#[test]
fn every_subcommand_reads_the_common_dictionary_from_the_variable() {
    let scratch = Scratch::new("variable");
    let dictionary = scratch.write("given/prh.yml", RULES);
    let home = scratch.0.join("home");
    std::fs::create_dir_all(&home).unwrap();
    for (name, out) in every_subcommand(&scratch, &home, Some(&dictionary)) {
        assert_eq!(out.status.code(), Some(1), "{name}: {}", stderr(&out));
        assert!(
            stdout(&out).contains("ふがふが => ほげ"),
            "{name}: {}",
            stdout(&out)
        );
    }
}

#[test]
fn every_subcommand_reads_the_common_dictionary_under_the_home() {
    // 規則を足した辞書を claude-setup sync が置く場所に置けば、ビルドし直さずに効く
    let scratch = Scratch::new("home");
    scratch.write("home/.config/ja-terms/prh.yml", RULES);
    let home = scratch.0.join("home");
    for (name, out) in every_subcommand(&scratch, &home, None) {
        assert_eq!(out.status.code(), Some(1), "{name}: {}", stderr(&out));
        assert!(
            stdout(&out).contains("ふがふが => ほげ"),
            "{name}: {}",
            stdout(&out)
        );
    }
}

#[test]
fn a_missing_common_dictionary_stops_every_subcommand_with_how_to_deliver_it() {
    let scratch = Scratch::new("missing");
    let home = scratch.0.join("home");
    std::fs::create_dir_all(&home).unwrap();
    for (name, out) in every_subcommand(&scratch, &home, None) {
        assert_eq!(out.status.code(), Some(2), "{name}: {}", stdout(&out));
        // commit-msg は理由を標準出力に、ほかは標準エラーに出す
        let why = format!("{}{}", stdout(&out), stderr(&out));
        assert!(why.contains("prh.yml"), "{name}: {why}");
        assert!(why.contains("claude-sync"), "{name}: {why}");
        assert!(!why.contains("ふがふが => ほげ"), "{name}: {why}");
    }
}

#[test]
fn a_missing_dictionary_at_the_variable_is_not_replaced_by_the_home_one() {
    // 環境変数で渡したパスに辞書が無ければ、ホームの辞書に黙ってすり替えない
    let scratch = Scratch::new("variable-missing");
    scratch.write("home/.config/ja-terms/prh.yml", RULES);
    let home = scratch.0.join("home");
    let missing = scratch.0.join("nonexistent/prh.yml");
    scratch.write("doc.md", "# 題\n\nふがふがの値\n");
    let out = ja_terms(
        &scratch,
        &scratch.0,
        &home,
        Some(&missing),
        &["check", "doc.md"],
        "",
    );
    assert_eq!(out.status.code(), Some(2), "{}", stdout(&out));
    assert!(stderr(&out).contains("nonexistent"), "{}", stderr(&out));
}

#[test]
fn an_unreadable_common_dictionary_stops_the_check() {
    // 新しい ja-terms のために書いた辞書を古い ja-terms が読めない（扱わない項目、specs が合わない）ときも止まる
    let broken = [
        ("yaml", "version: 1\nrules: [\n"),
        ("imports", "version: 1\nimports: [other.yml]\nrules: []\n"),
        (
            "specs",
            "version: 1\nrules:\n  - expected: ほげ\n    pattern: ふがふが\n    specs:\n      - from: ふがふが\n        to: ぴよ\n",
        ),
    ];
    for (name, yaml) in broken {
        let scratch = Scratch::new(name);
        let dictionary = scratch.write("given/prh.yml", yaml);
        scratch.write("doc.md", "# 題\n\nふがふがの値\n");
        let out = ja_terms(
            &scratch,
            &scratch.0,
            &scratch.0,
            Some(&dictionary),
            &["check", "doc.md"],
            "",
        );
        assert_eq!(out.status.code(), Some(2), "{name}: {}", stdout(&out));
        assert!(stderr(&out).contains("prh.yml"), "{name}: {}", stderr(&out));
    }
}

#[test]
fn a_given_dictionary_does_not_need_the_common_one() {
    // --dictionary は渡した辞書だけを当てる（ADR-0096）ので、共通の辞書が無くても検査できる
    let scratch = Scratch::new("given");
    let given = scratch.write("given.yml", RULES);
    scratch.write("doc.md", "# 題\n\nふがふがの値\n");
    let home = scratch.0.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let out = ja_terms(
        &scratch,
        &scratch.0,
        &home,
        None,
        &["check", "--dictionary", given.to_str().unwrap(), "doc.md"],
        "",
    );
    assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
    assert!(
        stdout(&out).contains("ふがふが => ほげ"),
        "{}",
        stdout(&out)
    );
}
