//! `history-rewrite run` が一時のリポジトリの履歴の語を直し、番号を付け直し、照合を通すこと（ADR-0045、ADR-0088）。
//!
//! 第 2 段の `commit-refs` は PATH のものを使う（Windows の CI は cargo install で入れ、Nix のビルドは
//! nativeCheckInputs で渡す）。git の全体設定は、テストの HOME の .gitconfig で与える。辞書の語（崩れた語）を
//! わざと含むので、.textlintignore で ja-terms の検査から外す。

use history_rewrite::run::{Verifier, read_map};
use history_rewrite::{Stop, Transformer, Words};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

fn scratch(name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "run-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("home")).unwrap();
    std::fs::write(
        root.join("home").join(".gitconfig"),
        "[init]\n\tdefaultBranch = main\n[user]\n\tname = t\n\temail = t@t\n[commit]\n\tgpgsign = false\n[tag]\n\tgpgsign = false\n",
    )
    .unwrap();
    root
}

fn env<'a>(cmd: &'a mut Command, root: &Path) -> &'a mut Command {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(key);
        }
    }
    cmd.env("HOME", root.join("home"))
        .env("USERPROFILE", root.join("home"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
}

struct Repo {
    root: PathBuf,
    path: PathBuf,
}

impl Repo {
    fn new(root: &Path, name: &str) -> Repo {
        let repo = Repo {
            root: root.to_path_buf(),
            path: root.join(name),
        };
        std::fs::create_dir_all(&repo.path).unwrap();
        repo.git(&["init", "-q"]);
        repo
    }

    fn run_git(&self, args: &[&str], input: Option<&[u8]>) -> Vec<u8> {
        use std::io::Write;
        let mut child = env(&mut Command::new("git"), &self.root)
            .arg("-C")
            .arg(&self.path)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.unwrap_or_default())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out.stdout
    }

    fn git(&self, args: &[&str]) -> String {
        String::from_utf8(self.run_git(args, None)).unwrap()
    }

    fn git_in(&self, args: &[&str], input: &str) -> String {
        String::from_utf8(self.run_git(args, Some(input.as_bytes()))).unwrap()
    }

    fn write(&self, name: &str, content: &[u8]) {
        let path = self.path.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    /// 作業ツリーの変更をすべてコミットする（index だけに置いた symlink の link.md は残す）
    fn commit(&self, message: &str) -> String {
        self.git(&["add", "-A", "--", ".", ":!link.md"]);
        self.git_in(&["commit", "-q", "--no-verify", "-F", "-"], message);
        self.git(&["rev-parse", "HEAD"]).trim().to_string()
    }

    fn show(&self, rev: &str, path: &str) -> Vec<u8> {
        self.run_git(&["show", &format!("{rev}:{path}")], None)
    }

    fn show_text(&self, rev: &str, path: &str) -> String {
        String::from_utf8(self.show(rev, path)).unwrap()
    }
}

fn history_rewrite(root: &Path, args: &[&str]) -> Output {
    env(
        &mut Command::new(env!("CARGO_BIN_EXE_history-rewrite")),
        root,
    )
    .args(args)
    .output()
    .unwrap()
}

fn run(root: &Path, source: &Path, workdir: &Path, words: &[&str]) -> Output {
    let (source, workdir) = (source.display().to_string(), workdir.display().to_string());
    let mut args = vec!["run", "--source", &source, "--workdir", &workdir];
    args.extend(words);
    history_rewrite(root, &args)
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// 語と番号を直す文書、手で直したコミット、ブランチ、タグ（署名つき、tagger の無いもの）、符号化を宣言した本文を
/// 持つ履歴を、1 度だけ書き換えた結果
struct Fixture {
    root: PathBuf,
    repo: Repo,
    rewritten: Repo,
    workdir: PathBuf,
    extra: PathBuf,
    first: String,
    second: String,
    manual: String,
    side: String,
    extra_tip: String,
    refs_before: String,
    out: HashMap<String, String>,
    status: Option<i32>,
    stderr: String,
    map: HashMap<String, String>,
}

fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let root = scratch("fixture");
        let repo = Repo::new(&root, "source");
        repo.write(
            "README.md",
            "# 手順\n\n確かめの結果を書く。結果を確かめる。\n\n| 項目 | 内容 |\n| ---- | ---- |\n| 幅 | 掃引の幅 |\n".as_bytes(),
        );
        repo.write("tool.py", "# 確かめの順に掃引する\nprint('ok')\n".as_bytes());
        let mut binary = b"\0\x01".to_vec();
        binary.extend_from_slice("確かめの".as_bytes());
        repo.write("data.bin", &binary);
        repo.write("dict/terms.yml", "pattern: 確かめの\n".as_bytes());
        // symlink は index だけに置く（Windows の作業ツリーでは作れない）
        let target = repo.git_in(&["hash-object", "-w", "--stdin"], "README.md");
        repo.git(&["update-index", "--add", "--cacheinfo", &format!("120000,{},link.md", target.trim())]);
        let first = repo.commit("docs: 確かめの手順を書く\n");

        repo.write(
            "notes.md",
            format!("手順は {} と {first} にある。旧称を使う。\n", &first[..8]).as_bytes(),
        );
        let second = repo.commit(&format!("docs: 記録を足す\n\n{} の確かめの結果。\n", &first[..7]));

        // 語を手で直しただけのコミット（変換の後は、親と同じ中身の空のコミットになる）
        repo.write("tool.py", "# 確認の順に掃引する\nprint('ok')\n".as_bytes());
        let manual = repo.commit("docs: 語を直す\n");

        repo.git(&["switch", "-q", "-c", "side", &second]);
        repo.write("side.md", format!("{} の後の確かめの記録\n", &manual[..10]).as_bytes());
        let side = repo.commit("docs: 別のブランチ\n");
        repo.git(&["tag", "-a", "v1", "-m", &format!("確かめの版 {}", &side[..7]), &side]);
        repo.git(&["tag", "light", &first]);
        // 署名つきのタグ（fast-export は署名を外す）と、tagger の無いタグ（fast-export は仮の tagger を付ける）
        repo.git(&[
            "tag",
            "-a",
            "signed",
            "-m",
            "確かめの版\n-----BEGIN PGP SIGNATURE-----\n\nAAAA\n-----END PGP SIGNATURE-----",
            &side,
        ]);
        let raw = format!("object {side}\ntype commit\ntag bare\n\n確かめの版\n");
        let bare = repo.git_in(&["hash-object", "-t", "tag", "-w", "--literally", "--stdin"], &raw);
        repo.git(&["update-ref", "refs/tags/bare", bare.trim()]);

        repo.git(&["switch", "-q", "-c", "extra", &manual]);
        repo.write("latin1.txt", b"caf\xe9 \xe7a\n");
        repo.write("fence.md", "```text\n| a | b |\n| - | - |\n| 掃引 | x |\n```\n".as_bytes());
        repo.write("crlf.md", "| a | b |\r\n| - | - |\r\n| 掃引 | x |\r\n".as_bytes());
        repo.git(&["add", "-A", "--", ".", ":!link.md"]);
        // 宣言した符号化の本文（直す語が無ければ、UTF-8 として読めなくても通る）
        let message = root.join("latin1-message.txt");
        std::fs::write(&message, b"docs: caf\xe9\n").unwrap();
        repo.git(&[
            "-c",
            "i18n.commitEncoding=ISO-8859-1",
            "commit",
            "-q",
            "--no-verify",
            "-F",
            &message.display().to_string(),
        ]);
        let extra_tip = repo.git(&["rev-parse", "HEAD"]).trim().to_string();
        repo.git(&["switch", "-q", "main"]);

        let refs_before = repo.git(&["for-each-ref", "--format=%(refname) %(objectname)"]);
        let extra = root.join("extra.yml");
        std::fs::write(&extra, "version: 1\nrules:\n  - expected: 新称\n    pattern: 旧称\n").unwrap();
        let workdir = root.join("work");
        let extra_s = extra.display().to_string();
        let result = run(
            &root,
            &repo.path,
            &workdir,
            &["--extra-rules", &extra_s, "--exclude", "**/terms.yml"],
        );
        let out = text(&result.stdout)
            .lines()
            .filter_map(|l| l.split_once(": "))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let map = if result.status.success() {
            read_map(&workdir.join("commit-map")).unwrap()
        } else {
            HashMap::new()
        };
        let rewritten = Repo {
            root: root.clone(),
            path: workdir.join("rewritten"),
        };
        Fixture {
            root,
            repo,
            rewritten,
            workdir,
            extra,
            first,
            second,
            manual,
            side,
            extra_tip,
            refs_before,
            out,
            status: result.status.code(),
            stderr: text(&result.stderr),
            map,
        }
    })
}

fn rewritten() -> &'static Fixture {
    let f = fixture();
    assert_eq!(f.status, Some(0), "{}", f.stderr);
    f
}

#[test]
fn words_are_fixed_in_documents_code_and_tables() {
    let f = rewritten();
    let readme = f.rewritten.show_text("main", "README.md");
    assert!(
        readme.contains("確認の結果を書く。結果を確かめる。"),
        "{readme}"
    );
    assert!(readme.contains("| 幅   | スイープの幅 |"), "{readme}");
    assert!(readme.contains("| ---- | ------------ |"), "{readme}");
    assert_eq!(
        f.rewritten.show_text(&f.map[&f.first], "tool.py"),
        "# 確認の順にスイープする\nprint('ok')\n"
    );
}

#[test]
fn extra_rules_apply_with_the_dictionary() {
    let f = rewritten();
    assert!(
        f.rewritten
            .show_text("main", "notes.md")
            .contains("新称を使う")
    );
}

#[test]
fn hashes_in_documents_and_messages_point_at_the_final_commits() {
    let f = rewritten();
    let first = &f.map[&f.first];
    let notes = f.rewritten.show_text("main", "notes.md");
    assert!(
        notes.contains(&format!("手順は {} と {first} にある", &first[..8])),
        "{notes}"
    );
    let body = f
        .rewritten
        .git(&["log", "-1", "--format=%B", &f.map[&f.second]]);
    assert!(
        body.contains(&format!("{} の確認の結果。", &first[..7])),
        "{body}"
    );
    assert!(
        f.rewritten
            .show_text("side", "side.md")
            .contains(&f.map[&f.manual][..10])
    );
    assert_eq!(
        f.rewritten.git(&["log", "-1", "--format=%s", first]).trim(),
        "docs: 確認の手順を書く"
    );
}

#[test]
fn every_original_commit_maps_to_a_commit_of_the_rewritten_history() {
    let f = rewritten();
    let mut commits: Vec<String> = f
        .repo
        .git(&["rev-list", "--all"])
        .split_whitespace()
        .map(String::from)
        .collect();
    commits.sort();
    let mut mapped: Vec<String> = f.map.keys().cloned().collect();
    mapped.sort();
    assert_eq!(mapped, commits);
    // 元の ref は commit-refs が refs/commit-refs/original/ に残すので、ブランチとタグだけを見る
    let mut reachable: Vec<String> = f
        .rewritten
        .git(&["rev-list", "--branches", "--tags"])
        .split_whitespace()
        .map(String::from)
        .collect();
    reachable.sort();
    let mut news: Vec<String> = f.map.values().cloned().collect();
    news.sort();
    news.dedup();
    assert_eq!(news, reachable);
    assert_eq!(f.out["COMMITS"], commits.len().to_string());
}

#[test]
fn branches_and_tags_move_to_the_mapped_commits() {
    let f = rewritten();
    let at = |rev: &str| f.rewritten.git(&["rev-parse", rev]).trim().to_string();
    assert_eq!(at("main"), f.map[&f.manual]);
    assert_eq!(at("side"), f.map[&f.side]);
    assert_eq!(at("light"), f.map[&f.first]);
    assert_eq!(at("v1^{commit}"), f.map[&f.side]);
    let message = f
        .rewritten
        .git(&["tag", "-l", "--format=%(contents)", "v1"]);
    assert_eq!(message.trim(), format!("確認の版 {}", &f.map[&f.side][..7]));
}

#[test]
fn signed_and_taggerless_tags_follow_fast_export() {
    let f = rewritten();
    let signed = f.rewritten.git(&["cat-file", "tag", "signed"]);
    assert!(signed.ends_with("\n\n確認の版\n"), "{signed}");
    let bare = f.rewritten.git(&["cat-file", "tag", "bare"]);
    assert!(
        bare.contains(&format!("object {}\n", f.map[&f.side])),
        "{bare}"
    );
    assert!(
        bare.contains("\ntagger Unspecified Tagger <unspecified-tagger> 0 +0000\n"),
        "{bare}"
    );
    assert!(bare.ends_with("\n\n確認の版\n"), "{bare}");
}

#[test]
fn a_message_in_a_declared_non_utf8_encoding_is_kept() {
    let f = rewritten();
    let raw = f
        .rewritten
        .run_git(&["cat-file", "commit", &f.map[&f.extra_tip]], None);
    let raw_text = String::from_utf8_lossy(&raw);
    assert!(raw_text.contains("\nencoding ISO-8859-1\n"), "{raw_text}");
    assert!(raw.ends_with(b"\n\ndocs: caf\xe9\n"), "{raw_text}");
}

#[test]
fn text_that_is_not_utf8_is_counted_once_and_kept() {
    let f = rewritten();
    assert_eq!(f.out["NOT_UTF8"], "1");
    assert_eq!(
        f.rewritten.show(&f.map[&f.extra_tip], "latin1.txt"),
        f.repo.show(&f.extra_tip, "latin1.txt")
    );
}

#[test]
fn tables_in_code_fences_are_not_realigned_and_crlf_is_kept() {
    let f = rewritten();
    let tip = &f.map[&f.extra_tip];
    assert_eq!(
        f.rewritten.show_text(tip, "fence.md"),
        "```text\n| a | b |\n| - | - |\n| スイープ | x |\n```\n"
    );
    let crlf = f.rewritten.show_text(tip, "crlf.md");
    assert_eq!(crlf.matches("\r\n").count(), 3, "{crlf:?}");
    assert!(crlf.contains("スイープ"));
}

#[test]
fn binary_symlink_and_excluded_files_are_kept() {
    let f = rewritten();
    for path in ["data.bin", "link.md", "dict/terms.yml"] {
        assert_eq!(
            f.rewritten.show(&f.map[&f.first], path),
            f.repo.show(&f.first, path),
            "{path}"
        );
    }
}

#[test]
fn the_counts_name_the_fixed_blobs_and_messages() {
    let f = rewritten();
    // README.md、tool.py の 2 つの版（手で直した版にも「掃引」が残る）、notes.md、side.md、fence.md、crlf.md の 7 つの
    // ブロブ。本文は 2 つのコミットと 3 つのタグ（Python の版の出力と同じ数）
    assert_eq!(f.out["BLOBS"], "7");
    assert_eq!(f.out["MESSAGES"], "5");
    assert_eq!(
        f.out["ORIGINAL"],
        f.workdir.join("original.git").display().to_string()
    );
    assert_eq!(
        f.out["MAP"],
        f.workdir.join("commit-map").display().to_string()
    );
}

#[test]
fn the_source_repository_is_not_touched() {
    let f = rewritten();
    assert_eq!(
        f.repo
            .git(&["for-each-ref", "--format=%(refname) %(objectname)"]),
        f.refs_before
    );
}

fn words(f: &Fixture, extra: &[PathBuf]) -> Words {
    let mut extra_rules = vec![f.extra.clone()];
    extra_rules.extend_from_slice(extra);
    Words {
        dictionaries: Vec::new(),
        extra_rules,
        excludes: vec!["**/terms.yml".to_string()],
    }
}

fn stage1_map(f: &Fixture) -> HashMap<String, String> {
    read_map(
        &f.rewritten
            .path
            .join(".git")
            .join("history-rewrite")
            .join("commit-map"),
    )
    .unwrap()
}

#[test]
fn verification_rejects_a_history_that_differs_from_the_transform() {
    // 第 1 段の結果を、ブロブを 1 つ余分に直す変換と突き合わせると、照合が止める
    let f = rewritten();
    let rules = f.root.join("rules-項目.yml");
    std::fs::write(
        &rules,
        "version: 1\nrules:\n  - expected: 内容\n    pattern: 項目\n",
    )
    .unwrap();
    let mut transformer = Transformer::new(&words(f, &[rules])).unwrap();
    let map = stage1_map(f);
    let Err(Stop(why)) = Verifier::new(
        &f.rewritten.path,
        &f.workdir.join("original.git"),
        &map,
        &mut transformer,
    )
    .run() else {
        panic!("照合が通ってしまった");
    };
    assert!(why.contains("README.md"), "{why}");
}

#[test]
fn verification_rejects_a_map_that_misses_an_original_commit() {
    let f = rewritten();
    let mut transformer = Transformer::new(&words(f, &[])).unwrap();
    let mut map = stage1_map(f);
    map.remove(&f.second);
    let Err(Stop(why)) = Verifier::new(
        &f.rewritten.path,
        &f.workdir.join("original.git"),
        &map,
        &mut transformer,
    )
    .run() else {
        panic!("照合が通ってしまった");
    };
    assert!(why.contains(&f.second), "{why}");
}

#[test]
fn a_workdir_inside_the_source_or_a_file_stops() {
    let f = fixture();
    let root = scratch("workdir");
    let file = root.join("afile");
    std::fs::write(&file, "").unwrap();
    for workdir in [f.repo.path.join("work"), file] {
        let out = run(&root, &f.repo.path, &workdir, &[]);
        assert_eq!(out.status.code(), Some(1), "{}", workdir.display());
        assert!(
            text(&out.stderr).starts_with("ERROR:"),
            "{}",
            text(&out.stderr)
        );
    }
    assert!(!f.repo.path.join("work").exists());
}

#[test]
fn a_non_empty_workdir_stops_without_rewriting() {
    let f = rewritten();
    let out = run(&f.root, &f.repo.path, &f.workdir, &[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("空でない"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn a_tag_of_a_tag_stops_before_rewriting() {
    // fast-export は入れ子のタグを平らにする（外のタグの名前で内のタグを作り直す）ので、照合できる形でない
    let root = scratch("nested");
    let repo = Repo::new(&root, "repo");
    repo.write("a.md", b"a\n");
    repo.commit("docs: a\n");
    repo.git(&["tag", "-a", "inner", "-m", "inner", "HEAD"]);
    repo.git(&[
        "-c",
        "advice.nestedTag=false",
        "tag",
        "-a",
        "outer",
        "-m",
        "outer",
        "inner",
    ]);
    let out = run(&root, &repo.path, &root.join("work"), &[]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stdout));
    let err = text(&out.stderr);
    assert!(
        err.contains("refs/tags/outer") && err.contains("git tag -f outer outer^{}"),
        "{err}"
    );
    assert!(!text(&out.stdout).contains("REWRITTEN:"));
}

#[test]
fn a_commit_header_that_fast_export_drops_is_named() {
    // fast-export は知らない見出し（jj の change-id など）を書き出さない
    let root = scratch("headers");
    let repo = Repo::new(&root, "repo");
    repo.write("a.md", b"a\n");
    let plain = repo.commit("docs: a\n");
    let raw = repo.git(&["cat-file", "commit", &plain]);
    let (head, body) = raw.split_once("\n\n").unwrap();
    let extra = repo.git_in(
        &["hash-object", "-t", "commit", "-w", "--stdin"],
        &format!("{head}\nchange-id zzzz\n\n{body}"),
    );
    repo.git(&["update-ref", "refs/heads/main", extra.trim()]);
    let out = run(&root, &repo.path, &root.join("work"), &[]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stdout));
    assert!(
        text(&out.stderr).contains("change-id"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn a_malformed_commit_map_stops() {
    let root = scratch("map");
    let path = root.join("bad-map");
    std::fs::write(&path, "old new\naaaa bbbb cccc\n").unwrap();
    let Err(Stop(why)) = read_map(&path) else {
        panic!("読めてしまった");
    };
    assert!(why.contains("bad-map") && why.contains(":2:"), "{why}");
}

#[test]
fn a_message_in_a_declared_non_utf8_encoding_that_would_change_stops() {
    let root = scratch("sjis");
    let repo = Repo::new(&root, "repo");
    repo.write("a.md", b"a\n");
    repo.git(&["add", "a.md"]);
    // 本文は宣言した符号化のバイト列（git は -F の本文を変換せず、encoding の見出しを付けるだけ）。「確かめの記録」の Shift_JIS
    let message = root.join("sjis-message.txt");
    let mut bytes = b"docs: ".to_vec();
    bytes.extend_from_slice(b"\x8a\x6d\x82\xa9\x82\xdf\x82\xcc\x8b\x4c\x98\x5e\n");
    std::fs::write(&message, bytes).unwrap();
    repo.git(&[
        "-c",
        "i18n.commitEncoding=Shift_JIS",
        "commit",
        "-q",
        "--no-verify",
        "-F",
        &message.display().to_string(),
    ]);
    let out = run(&root, &repo.path, &root.join("work"), &[]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stdout));
    assert!(
        text(&out.stderr).contains("Shift_JIS"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn a_message_table_that_cannot_be_realigned_names_the_commit() {
    let root = scratch("message-table");
    let repo = Repo::new(&root, "repo");
    repo.write("a.md", b"a\n");
    let commit = repo.commit("docs: a\n\n| a | b |\n| - | - |\n| x \\| y | 掃引 |\n");
    let out = run(&root, &repo.path, &root.join("work"), &[]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stdout));
    let err = text(&out.stderr);
    assert!(err.contains(&commit), "{err}");
    assert!(!err.contains("--exclude"), "{err}");
}

#[test]
fn replace_refs_of_the_source_do_not_change_the_original_commits() {
    // clone --mirror は refs/replace/ も写すが、第 1 段に渡すのはブランチとタグだけ
    let root = scratch("replaced");
    let repo = Repo::new(&root, "repo");
    repo.write("a.md", b"a\n");
    let first = repo.commit("docs: a\n");
    repo.write("a.md", b"b\n");
    repo.commit("docs: b\n");
    // 置き換えで足した親は、ブランチからもタグからも届かない
    let tree = repo.git(&["rev-parse", "HEAD^{tree}"]);
    let orphan = repo.git(&["commit-tree", "-m", "docs: x", tree.trim()]);
    repo.git(&["replace", "--graft", &first, orphan.trim()]);
    let out = run(&root, &repo.path, &root.join("work"), &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
}

#[test]
fn a_quoted_path_is_fixed_by_its_real_name() {
    // fast-export は特別な字（ASCII でない字を含む）を含むパスを引用する。.md の判定と --exclude は引用を外したパスで
    // 行う（`"` などのエスケープは unquote の単体テスト。Windows では `"` を含む名前のファイルを作れない）
    let root = scratch("quoted");
    let repo = Repo::new(&root, "repo");
    repo.write(
        "表 の記録.md",
        "| a | b |\n| - | - |\n| 掃引 | x |\n".as_bytes(),
    );
    repo.write("確かめ.md", "確かめの記録\n".as_bytes());
    repo.commit("docs: a\n");
    let workdir = root.join("work");
    let out = run(&root, &repo.path, &workdir, &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let rewritten = Repo {
        root: root.clone(),
        path: workdir.join("rewritten"),
    };
    assert_eq!(
        rewritten.show_text("main", "表 の記録.md"),
        "| a        | b |\n| -------- | - |\n| スイープ | x |\n"
    );
    assert_eq!(rewritten.show_text("main", "確かめ.md"), "確認の記録\n");
}

#[test]
fn usage_errors_exit_2() {
    let root = scratch("usage");
    assert_eq!(history_rewrite(&root, &["run"]).status.code(), Some(2));
    assert_eq!(
        history_rewrite(&root, &["run", "--source", "x"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        history_rewrite(&root, &["run", "--source", "x", "--workdir", "y", "z"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(history_rewrite(&root, &[]).status.code(), Some(2));
}

#[test]
fn encoding_names_that_git_and_iconv_accept_are_read() {
    // git は encoding の見出しの名前を iconv に渡す。WHATWG の名前に無い書き方（eucJP、euc_jp、cp932）も読む
    for name in ["eucJP", "euc_jp", "cp932", "latin1"] {
        let root = scratch("encoding");
        let repo = Repo::new(&root, "repo");
        repo.write("a.md", b"a\n");
        repo.git(&["add", "a.md"]);
        let message = root.join("message.txt");
        std::fs::write(&message, b"docs: plain\n").unwrap();
        repo.git(&[
            "-c",
            &format!("i18n.commitEncoding={name}"),
            "commit",
            "-q",
            "--no-verify",
            "-F",
            &message.display().to_string(),
        ]);
        let out = run(&root, &repo.path, &root.join("work"), &[]);
        assert_eq!(out.status.code(), Some(0), "{name}: {}", text(&out.stderr));
    }
}

#[test]
fn a_stop_in_the_first_stage_leaves_no_crash_report_of_fast_import() {
    let root = scratch("crash");
    let repo = Repo::new(&root, "repo");
    repo.write(
        "a.md",
        "| a | b |\n| - | - |\n| x \\| y | 掃引 |\n".as_bytes(),
    );
    repo.commit("docs: a\n");
    let workdir = root.join("work");
    let out = run(&root, &repo.path, &workdir, &[]);
    assert_eq!(out.status.code(), Some(1));
    let err = text(&out.stderr);
    assert!(err.contains("\\|"), "{err}");
    assert!(!err.contains("fatal:"), "{err}");
    let git_dir = workdir.join("rewritten").join(".git");
    let crashes: Vec<_> = std::fs::read_dir(&git_dir)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("fast_import_crash")
        })
        .collect();
    assert!(crashes.is_empty(), "{crashes:?}");
}

#[test]
fn a_latin1_message_with_c1_bytes_is_read() {
    // ISO-8859-1 は 0x80〜0x9f も 1 字に読む（WHATWG の windows-1252 は 0x81、0x8d、0x8f、0x90、0x9d を持たない）。iconv の
    // 別名（csISOLatin1、ISO_8859-1:1987、ISO-IR-100）も同じ
    for name in ["ISO-8859-1", "csISOLatin1", "ISO_8859-1:1987", "ISO-IR-100"] {
        let root = scratch("c1");
        let repo = Repo::new(&root, "repo");
        repo.write("a.md", b"a\n");
        repo.git(&["add", "a.md"]);
        let message = root.join("message.txt");
        std::fs::write(&message, b"docs: \x81\x8d\x90\n").unwrap();
        repo.git(&[
            "-c",
            &format!("i18n.commitEncoding={name}"),
            "commit",
            "-q",
            "--no-verify",
            "-F",
            &message.display().to_string(),
        ]);
        let out = run(&root, &repo.path, &root.join("work"), &[]);
        assert_eq!(out.status.code(), Some(0), "{name}: {}", text(&out.stderr));
    }
}
