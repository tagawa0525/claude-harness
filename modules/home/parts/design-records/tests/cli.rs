//! CLI を一時的な git リポジトリで走らせ、雛形、検査、status の読み出しを確かめる。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use std::io::Write;

/// テストで作る一時的なリポジトリ。落ちたときも含めて、片付けで消す。作業ツリーは `root/work` に置き、bare の
/// リモートとほかの clone は `root` の下に並べる（作業ツリーの中に置くと `git add -A` が拾う）。
struct TempRepo {
    root: PathBuf,
    dir: PathBuf,
}

impl TempRepo {
    /// プロセスと時刻とテストの名前で一意にした、空のコミットを 1 つ持つ main ブランチのリポジトリ。CLI が作る
    /// コミットとタグのために、名前とメールをリポジトリの設定に書く
    fn new(name: &str) -> Self {
        let repo = Self::empty(name);
        repo.git(&["commit", "-q", "--allow-empty", "-m", "chore: init"]);
        repo
    }

    /// コミットの無い main ブランチのリポジトリ
    fn empty(name: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "design-records-{name}-{}-{nanos}",
            std::process::id()
        ));
        let dir = root.join("work");
        fs::create_dir_all(&dir).unwrap();
        let repo = TempRepo { root, dir };
        repo.git(&["init", "-q", "-b", "main"]);
        configure(&repo.dir);
        repo
    }

    /// `root/remote.git` を bare のリモート origin にし、main を送る
    fn with_remote(name: &str) -> Self {
        let repo = Self::new(name);
        let bare = repo.root.join("remote.git");
        git_in(&repo.root, &["init", "-q", "--bare", "remote.git"]);
        repo.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
        repo.git(&["push", "-q", "-u", "origin", "main"]);
        repo
    }

    /// origin を `root/<name>` に clone する
    fn clone_remote(&self, name: &str) -> PathBuf {
        git_in(&self.root, &["clone", "-q", "remote.git", name]);
        let dir = self.root.join(name);
        configure(&dir);
        dir
    }

    /// タグ `name` のメッセージの 1 行目（注釈つきでなければ空）
    fn tag_message(&self, name: &str) -> String {
        tag_message_in(&self.dir, name)
    }

    /// origin のタグ `name` の行（無ければ空）
    fn remote_tag(&self, name: &str) -> String {
        self.git(&[
            "ls-remote",
            "--tags",
            "origin",
            &format!("refs/tags/{name}"),
        ])
    }

    /// git を走らせ、成功を確かめて標準出力を返す
    fn git(&self, args: &[&str]) -> String {
        git_in(&self.dir, args)
    }

    fn write(&self, path: &str, text: &str) {
        let file = self.dir.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }

    fn read(&self, path: &str) -> String {
        fs::read_to_string(self.dir.join(path)).unwrap()
    }

    fn commit_all(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
    }

    /// CLI をリポジトリの `sub`（直下なら ""）で走らせる
    fn run_in(&self, sub: &str, args: &[&str]) -> Output {
        run(&self.dir.join(sub), args, None)
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_in("", args)
    }

    /// CLI を、`bin` を PATH の先頭に置いて走らせる（git を差し替える）
    fn run_with_bin(&self, bin: &Path, args: &[&str]) -> Output {
        let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
        let mut command = Command::new(env!("CARGO_BIN_EXE_design-records"));
        command.current_dir(&self.dir).env("PATH", path).args(args);
        isolate(&mut command);
        command.output().unwrap()
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        // 片付けに失敗しても、テストの結果は変えない（一時的なディレクトリが残るだけ）
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// git を `dir` で走らせ、成功を確かめて標準出力を返す。親の git（hook）から受け継いだ GIT_ で始まる環境変数は外す
/// （外さないと、hook の中で走るテストの操作が、hook を呼んだ本物のリポジトリに効く）
fn git_in(dir: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    command
        .current_dir(dir)
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args);
    isolate(&mut command);
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// CLI が作るコミットとタグのための、リポジトリの設定（署名と hook は使わない）
fn configure(dir: &Path) {
    for (key, value) in [
        ("user.name", "t"),
        ("user.email", "t@t"),
        ("commit.gpgsign", "false"),
        ("tag.gpgsign", "false"),
        ("core.hooksPath", "/dev/null"),
    ] {
        git_in(dir, &["config", key, value]);
    }
}

fn tag_message_in(dir: &Path, name: &str) -> String {
    git_in(
        dir,
        &[
            "for-each-ref",
            "--format=%(contents:subject)",
            &format!("refs/tags/{name}"),
        ],
    )
    .trim()
    .to_string()
}

fn isolate(command: &mut Command) {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            command.env_remove(key);
        }
    }
}

fn run(dir: &Path, args: &[&str], stdin: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_design-records"));
    command
        .current_dir(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    isolate(&mut command);
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.unwrap_or("").as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn today() -> String {
    let output = Command::new("date").arg("+%F").output().unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

#[test]
fn new_writes_the_template_and_prints_its_path() {
    let t = TempRepo::new("new");
    let out = t.run(&["new", "use-foo", "foo を使う"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "docs/adr/0001-use-foo.md\n");
    assert_eq!(
        t.read("docs/adr/0001-use-foo.md"),
        design_records::template(1, "foo を使う", &today())
    );
}

#[test]
fn new_writes_under_the_repository_root_from_a_subdirectory() {
    let t = TempRepo::new("subdir");
    t.write("src/deep/.keep", "");
    let out = t.run_in("src/deep", &["new", "use-bar", "bar を使う"]);
    assert_eq!(stdout(&out), "docs/adr/0001-use-bar.md\n");
    assert!(t.dir.join("docs/adr/0001-use-bar.md").exists());
}

#[test]
fn new_numbers_after_deleted_uncommitted_and_other_branch_adrs() {
    let t = TempRepo::new("numbers");
    t.write("docs/adr/0001-keep-a.md", "");
    t.write("docs/adr/0003-drop-c.md", "");
    t.commit_all("docs(adr): a and c");
    t.git(&["rm", "-q", "docs/adr/0003-drop-c.md"]);
    t.commit_all("docs(adr): drop c");
    // 履歴にだけ残る削除済みの ADR
    assert_eq!(
        stdout(&t.run(&["new", "use-d", "d を使う"])),
        "docs/adr/0004-use-d.md\n"
    );
    // まだコミットしていない ADR
    assert_eq!(
        stdout(&t.run(&["new", "use-e", "e を使う"])),
        "docs/adr/0005-use-e.md\n"
    );
    t.git(&["stash", "-q", "-u"]);
    t.git(&["switch", "-q", "-c", "other"]);
    t.write("docs/adr/0009-on-other.md", "");
    t.commit_all("docs(adr): on other branch");
    t.git(&["switch", "-q", "main"]);
    // 別のブランチにだけコミットされた ADR
    assert_eq!(
        stdout(&t.run(&["new", "use-h", "h を使う"])),
        "docs/adr/0010-use-h.md\n"
    );
}

#[test]
fn new_continues_three_digit_numbers_in_four_digits() {
    let t = TempRepo::new("three");
    t.write("docs/adr/001-keep-a.md", "");
    t.write("docs/adr/007-keep-g.md", "");
    t.commit_all("docs(adr): three digits");
    assert_eq!(
        stdout(&t.run(&["new", "use-i", "i を使う"])),
        "docs/adr/0008-use-i.md\n"
    );
}

#[test]
fn new_rejects_bad_slugs_and_missing_titles_without_writing() {
    let t = TempRepo::new("bad");
    let out = t.run(&["new", "Use_F", "f を使う"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("kebab-case"), "{}", stderr(&out));
    let out = t.run(&["new", "use-g"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("Usage:"), "{}", stderr(&out));
    for title in ["", "  ", "a\nb"] {
        let out = t.run(&["new", "use-g", title]);
        assert!(!out.status.success(), "{title:?}");
    }
    assert!(!t.dir.join("docs").exists());
}

#[test]
fn new_fails_without_writing_when_the_history_cannot_be_read() {
    let t = TempRepo::new("nohistory");
    // log だけを失敗させる git を PATH の先頭に置く
    let bin = t.dir.join(".fake-bin");
    fs::create_dir(&bin).unwrap();
    let real = String::from_utf8(
        Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let fake = bin.join("git");
    fs::write(
        &fake,
        format!(
            "#!/bin/sh\ncase \" $* \" in *\" log \"*) exit 128 ;; esac\nexec {} \"$@\"\n",
            real.trim()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let mut command = Command::new(env!("CARGO_BIN_EXE_design-records"));
    command
        .current_dir(&t.dir)
        .env("PATH", path)
        .args(["new", "use-j", "j を使う"]);
    isolate(&mut command);
    let out = command.output().unwrap();
    assert!(!out.status.success(), "{}", stdout(&out));
    assert!(!t.dir.join("docs").exists());
}

#[test]
fn new_fails_outside_a_git_repository() {
    let dir = std::env::temp_dir().join(format!("design-records-nogit-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    // 上の階層のリポジトリを見つけないように止める
    let out = Command::new(env!("CARGO_BIN_EXE_design-records"))
        .current_dir(&dir)
        .env("GIT_CEILING_DIRECTORIES", dir.parent().unwrap())
        .args(["new", "use-x", "x"])
        .output()
        .unwrap();
    let _ = fs::remove_dir_all(&dir);
    assert!(!out.status.success());
}

#[test]
fn check_passes_a_new_template_and_a_repository_without_adrs() {
    let t = TempRepo::new("clean");
    let out = t.run(&["check"]);
    assert!(out.status.success(), "{}", stdout(&out));
    t.run(&["new", "use-foo", "foo を使う"]);
    t.git(&["add", "-A"]);
    let out = t.run(&["check"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert_eq!(stdout(&out), "");
}

#[test]
fn check_fails_on_errors_and_lists_them() {
    let t = TempRepo::new("errors");
    t.run(&["new", "use-foo", "foo を使う"]);
    let path = "docs/adr/0001-use-foo.md";
    t.write(
        path,
        &t.read(path).replace("status: proposed", "status: approved"),
    );
    // 追跡していない ADR（コミットに入らない下書き）は見ない
    let out = t.run(&["check"]);
    assert!(out.status.success(), "{}", stdout(&out));
    t.git(&["add", "-A"]);
    let out = t.run(&["check"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stdout(&out).contains("docs/adr/0001-use-foo.md: error: `status` must be one of"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn check_with_files_shows_warnings_only_for_them_but_errors_for_all() {
    let t = TempRepo::new("scope");
    t.run(&["new", "use-a", "a を使う"]);
    t.run(&["new", "use-b", "b を使う"]);
    let a = "docs/adr/0001-use-a.md";
    let b = "docs/adr/0002-use-b.md";
    // どちらも推奨の節（確認）が無い。a は status も違反
    t.write(
        a,
        &t.read(a)
            .replace("### 確認\n", "")
            .replace("proposed", "approved"),
    );
    t.write(b, &t.read(b).replace("### 確認\n", ""));
    t.git(&["add", "-A"]);
    let out = t.run(&["check", b]);
    assert_eq!(out.status.code(), Some(1));
    let shown = stdout(&out);
    assert!(shown.contains(&format!("{a}: error: `status`")), "{shown}");
    assert!(
        shown.contains(&format!("{b}: warning: no section `確認`")),
        "{shown}"
    );
    assert!(!shown.contains(&format!("{a}: warning")), "{shown}");
    // FILE を渡さなければ、すべての ADR の warning を出す
    let shown = stdout(&t.run(&["check"]));
    assert!(
        shown.contains(&format!("{a}: warning: no section `確認`")),
        "{shown}"
    );
}

#[test]
fn check_changed_without_files_shows_errors_but_no_warnings() {
    // pre-commit は、渡すファイルが無いコミット（ADR を消すだけなど）でも ADR どうしの整合を確かめる
    let t = TempRepo::new("changed");
    t.run(&["new", "use-a", "a を使う"]);
    let a = "docs/adr/0001-use-a.md";
    t.write(
        a,
        &t.read(a)
            .replace("### 確認\n", "")
            .replace("requires: []", "requires: [ADR-0009]"),
    );
    t.git(&["add", "-A"]);
    let out = t.run(&["check", "--changed"]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert_eq!(
        stdout(&out),
        format!("{a}: error: `requires` names ADR-0009, which does not exist\n")
    );
    // --changed はファイルではなくオプション。知らないオプションは使い方の誤り
    let out = t.run(&["check", "--bogus"]);
    assert_eq!(out.status.code(), Some(2), "{}", stdout(&out));
}

#[test]
fn check_counts_only_tracked_issues() {
    let t = TempRepo::new("issues");
    t.write("docs/issues/0001-bug.md", "---\nstatus: open\n---\n");
    t.run(&["new", "use-a", "a を使う"]);
    let a = "docs/adr/0001-use-a.md";
    t.write(a, &t.read(a).replace("issues: []", "issues: [\"#0012\"]"));
    t.git(&["add", "-A"]);
    // 追跡していない issue の下書きは、参照先として数えない
    t.write("docs/issues/0012-draft.md", "---\nstatus: open\n---\n");
    let out = t.run(&["check", a]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert!(
        stdout(&out).contains("`issues` names #0012, which does not exist in docs/issues"),
        "{}",
        stdout(&out)
    );
}

/// ADR-0001 を ADR-0002 で置き換えた main
fn superseded_repo(name: &str) -> TempRepo {
    let t = TempRepo::new(name);
    t.run(&["new", "use-a", "a を使う"]);
    t.run(&["new", "use-b", "b を使う"]);
    let a = "docs/adr/0001-use-a.md";
    let b = "docs/adr/0002-use-b.md";
    t.write(
        a,
        &t.read(a)
            .replace("status: proposed", "status: superseded")
            .replace("superseded-by:", "superseded-by: ADR-0002"),
    );
    t.write(
        b,
        &t.read(b)
            .replace("status: proposed", "status: accepted")
            .replace("supersedes: []", "supersedes: [ADR-0001]"),
    );
    t.commit_all("docs(adr): a and b");
    t
}

#[test]
fn check_warns_about_superseded_adrs_mentioned_in_the_given_files() {
    let t = superseded_repo("mentions");
    t.write("README.md", "ADR-0001 に従う\n");
    let out = t.run(&["check", "README.md", "docs/adr/0002-use-b.md"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert_eq!(
        stdout(&out),
        "README.md:1: warning: ADR-0001 is superseded by ADR-0002\n"
    );
}

#[test]
fn check_warns_when_the_body_of_a_decided_adr_on_main_changes() {
    let t = superseded_repo("body");
    t.git(&["switch", "-q", "-c", "topic"]);
    let b = "docs/adr/0002-use-b.md";
    t.write(b, &(t.read(b) + "\n本文を足す\n"));
    let out = t.run(&["check", b]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(
        stdout(&out).starts_with("docs/adr/0002-use-b.md: warning: the body changed"),
        "{}",
        stdout(&out)
    );
}

/// accepted の ADR-0001 だけを持つ main から分けた topic ブランチ（ADR-0001 の予約は解放済み）
fn decided_topic(name: &str) -> TempRepo {
    let t = TempRepo::new(name);
    t.run(&["new", "use-a", "a を使う"]);
    let a = "docs/adr/0001-use-a.md";
    t.write(
        a,
        &t.read(a).replace("status: proposed", "status: accepted"),
    );
    t.commit_all("docs(adr): a");
    // main に入った記録の予約は解放されている（post-merge-cleanup.sh）
    t.run(&["prune"]);
    t.git(&["switch", "-q", "-c", "topic"]);
    t
}

#[test]
fn check_warns_when_a_decided_adr_is_deleted() {
    let t = decided_topic("deleted");
    t.git(&["rm", "-q", "docs/adr/0001-use-a.md"]);
    // 消したファイルは pre-commit から渡されない
    let out = t.run(&["check", "--changed"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(
        stdout(&out).starts_with("docs/adr/0001-use-a.md: warning: the ADR was deleted"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn check_follows_renames_when_comparing_the_body() {
    let t = decided_topic("renamed");
    let new = "docs/adr/0001-use-aa.md";
    t.git(&["mv", "docs/adr/0001-use-a.md", new]);
    t.write(new, &(t.read(new) + "\n本文を足す\n"));
    t.git(&["add", "-A"]);
    let out = t.run(&["check", "--changed", new]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(
        stdout(&out).starts_with(&format!("{new}: warning: the body changed")),
        "{}",
        stdout(&out)
    );
}

#[test]
fn check_lets_adrs_change_freely_before_they_are_decided_on_main() {
    let t = superseded_repo("free");
    t.git(&["switch", "-q", "-c", "topic"]);
    // main では proposed
    t.run(&["new", "use-c", "c を使う"]);
    t.commit_all("docs(adr): c");
    let c = "docs/adr/0003-use-c.md";
    t.write(
        c,
        &t.read(c).replace("status: proposed", "status: accepted"),
    );
    t.commit_all("docs(adr): accept c");
    // ブランチの上で accepted にしたものも、main にマージする前は直してよい
    t.write(c, &(t.read(c) + "\n本文を足す\n"));
    // 確定した ADR の前付けと補足は直してよい
    let a = "docs/adr/0001-use-a.md";
    let old_date = format!("date: {}", today());
    t.write(
        a,
        &(t.read(a).replace(&old_date, "date: 2026-10-12") + "\n## 補足\n\n- 2026-10-12: 追記\n"),
    );
    let out = t.run(&["check", c, a]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert_eq!(stdout(&out), "");
}

#[test]
fn check_compares_bodies_even_when_the_base_front_matter_is_broken() {
    let t = superseded_repo("broken-base");
    let b = "docs/adr/0002-use-b.md";
    // main の ADR-0002 の前付けに空行がある（check の違反）
    t.write(b, &t.read(b).replacen("requires:", "\nrequires:", 1));
    t.commit_all("docs(adr): break b");
    t.git(&["switch", "-q", "-c", "topic"]);
    t.write(b, &t.read(b).replacen("\nrequires:", "requires:", 1));
    let out = t.run(&["check", b]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert_eq!(stdout(&out), "");
}

#[test]
fn check_skips_body_comparison_without_a_merge_base() {
    let t = superseded_repo("orphan");
    // main と履歴を共有しないブランチ（index と作業ツリーは引き継ぐ）
    t.git(&["checkout", "-q", "--orphan", "lone"]);
    t.git(&["commit", "-q", "-m", "docs(adr): lone"]);
    let b = "docs/adr/0002-use-b.md";
    t.write(b, &(t.read(b) + "\n本文を足す\n"));
    let out = t.run(&["check", b]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
}

#[test]
fn status_reads_the_front_matter_from_stdin() {
    let dir = std::env::temp_dir();
    let out = run(&dir, &["status"], Some("---\nstatus: accepted\n---\n# t\n"));
    assert_eq!(stdout(&out), "accepted\n");
    let out = run(&dir, &["status"], Some("# no front matter\n"));
    assert_eq!(stdout(&out), "-\n");
    let out = run(&dir, &["status"], Some("---\ndate: 2026-10-11\n---\n"));
    assert_eq!(stdout(&out), "-\n");
    // 前付けのほかの行の形は問わない（docs/issues などの、検査していない文書も読む）。形の検査は check が行う
    let lenient = "---\ntags:\n  - a\n\nrefs: [1, 2\nstatus: \"accepted\" # note\nstatus: b\n---\n";
    let out = run(&dir, &["status"], Some(lenient));
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "accepted\n");
}

#[test]
fn unknown_commands_show_usage() {
    let out = run(&std::env::temp_dir(), &["frobnicate"], None);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("Usage:"), "{}", stderr(&out));
}

/// タグ `name` が指すコミット
fn tag_target(dir: &Path, name: &str) -> String {
    git_in(dir, &["rev-parse", &format!("{name}^{{commit}}")])
        .trim()
        .to_string()
}

#[test]
fn new_reserves_the_number_with_an_annotated_tag_on_the_default_branch() {
    let t = TempRepo::new("reserve");
    let main = t.git(&["rev-parse", "main"]).trim().to_string();
    t.git(&["switch", "-q", "-c", "topic"]);
    t.git(&["commit", "-q", "--allow-empty", "-m", "chore: topic"]);
    let out = t.run(&["new", "use-foo", "foo を使う"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(t.git(&["cat-file", "-t", "adr-0001"]).trim(), "tag");
    assert_eq!(t.tag_message("adr-0001"), "use-foo");
    // ブランチの先端ではなく、既定ブランチの先端に打つ
    assert_eq!(tag_target(&t.dir, "adr-0001"), main);
}

#[test]
fn new_skips_numbers_reserved_by_tags_of_its_kind() {
    let t = TempRepo::new("reserved");
    t.git(&["tag", "-a", "adr-0004", "-m", "use-d", "main"]);
    t.git(&["tag", "-a", "issue-0009", "-m", "fix-i", "main"]);
    assert_eq!(
        stdout(&t.run(&["new", "use-e", "e を使う"])),
        "docs/adr/0005-use-e.md\n"
    );
    assert_eq!(
        stdout(&t.run(&["new", "--kind", "issue", "fix-j", "j を直す"])),
        "docs/issues/0010-fix-j.md\n"
    );
}

#[test]
fn new_fails_in_a_repository_without_commits() {
    let t = TempRepo::empty("nocommit");
    let out = t.run(&["new", "use-foo", "foo を使う"]);
    assert!(!out.status.success(), "{}", stdout(&out));
    assert!(!t.dir.join("docs").exists());
}

#[test]
fn new_pushes_the_reservation_after_fetching_tags_of_other_clones() {
    let t = TempRepo::with_remote("push");
    let other = t.clone_remote("other");
    git_in(
        &other,
        &["tag", "-a", "adr-0003", "-m", "use-c", "origin/main"],
    );
    git_in(&other, &["push", "-q", "origin", "refs/tags/adr-0003"]);
    let out = t.run(&["new", "use-d", "d を使う"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "docs/adr/0004-use-d.md\n");
    assert!(t.remote_tag("adr-0004").contains("refs/tags/adr-0004"));
    assert_eq!(tag_message_in(&other, "adr-0003"), "use-c");
}

#[test]
fn new_takes_the_next_number_when_another_clone_pushes_the_same_tag_first() {
    let t = TempRepo::with_remote("race");
    let other = t.clone_remote("other");
    git_in(
        &other,
        &["tag", "-a", "adr-0001", "-m", "use-other", "origin/main"],
    );
    // 最初の fetch の直後に、ほかの clone が同じ番号を push する git
    let bin = t.root.join("bin");
    fs::create_dir(&bin).unwrap();
    let real = String::from_utf8(
        Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let real = real.trim();
    let mark = t.root.join("raced");
    let fake = bin.join("git");
    fs::write(
        &fake,
        format!(
            "#!/bin/sh\n\
             case \" $* \" in *\" fetch \"*)\n\
             \x20 {real} \"$@\" || exit $?\n\
             \x20 if [ ! -e {mark} ]; then : > {mark}; {real} -C {other} push -q origin refs/tags/adr-0001 || exit 99; fi\n\
             \x20 exit 0 ;;\n\
             esac\n\
             exec {real} \"$@\"\n",
            mark = mark.display(),
            other = other.display(),
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    let out = t.run_with_bin(&bin, &["new", "use-foo", "foo を使う"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(mark.exists());
    assert_eq!(stdout(&out), "docs/adr/0002-use-foo.md\n");
    assert!(!t.dir.join("docs/adr/0001-use-foo.md").exists());
    assert_eq!(t.tag_message("adr-0001"), "use-other");
    assert_eq!(t.tag_message("adr-0002"), "use-foo");
    assert!(t.remote_tag("adr-0002").contains("refs/tags/adr-0002"));
}

#[test]
fn new_fails_without_writing_or_tagging_when_the_remote_is_unreachable() {
    let t = TempRepo::with_remote("unreachable");
    let gone = t.root.join("gone.git");
    t.git(&["remote", "set-url", "origin", gone.to_str().unwrap()]);
    let out = t.run(&["new", "use-foo", "foo を使う"]);
    assert!(!out.status.success(), "{}", stdout(&out));
    assert!(!t.dir.join("docs").exists());
    assert_eq!(t.git(&["tag", "--list"]), "");
}

#[test]
fn new_issue_reserves_a_number_and_writes_the_title() {
    let t = TempRepo::new("issue");
    t.write("docs/issues/0003-fix-c.md", "---\nstatus: open\n---\n");
    t.write("docs/adr/0007-use-g.md", "");
    t.commit_all("docs: c and g");
    let out = t.run(&["new", "--kind", "issue", "fix-bar", "bar を直す"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "docs/issues/0004-fix-bar.md\n");
    assert_eq!(t.read("docs/issues/0004-fix-bar.md"), "# bar を直す\n");
    assert_eq!(t.tag_message("issue-0004"), "fix-bar");
    let out = t.run(&["new", "--kind", "plan", "x", "x"]);
    assert_eq!(out.status.code(), Some(2), "{}", stderr(&out));
}

/// ブランチ topic で ADR-0001（use-b）を足した後、main にも機能より前の ADR-0001（use-a、タグ無し）が入り、
/// topic を main の上に rebase したリポジトリ
fn duplicated_after_rebase(name: &str) -> TempRepo {
    let t = TempRepo::new(name);
    t.write("notes.md", "前置き\n1\n2\n3\n");
    t.commit_all("docs: notes");
    t.git(&["switch", "-q", "-c", "topic"]);
    t.run(&["new", "use-b", "b を使う"]);
    let b = "docs/adr/0001-use-b.md";
    t.write(b, &(t.read(b) + "\n[本文](0001-use-b.md) と ADR-0001\n"));
    t.write("notes.md", "ADR-0001 の続き\n前置き\n1\n2\n3\n");
    t.commit_all("docs(adr): b");
    t.git(&["switch", "-q", "main"]);
    t.write(
        "docs/adr/0001-use-a.md",
        &design_records::template(1, "a を使う", "2026-10-01"),
    );
    t.write("notes.md", "前置き\n1\n2\n3\nADR-0001 は a\n");
    t.commit_all("docs(adr): a");
    t.git(&["switch", "-q", "topic"]);
    t.git(&["rebase", "-q", "main"]);
    t
}

#[test]
fn renumber_moves_the_branch_record_and_its_added_references_in_one_commit() {
    let t = duplicated_after_rebase("renumber");
    let out = t.run(&["check"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stdout(&out).contains("number 0001 is also used by"),
        "{}",
        stdout(&out)
    );
    let head = t.git(&["rev-parse", "HEAD"]);
    let out = t.run(&["renumber", "1"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert_eq!(stdout(&out), "docs/adr/0002-use-b.md\n");
    assert!(!t.dir.join("docs/adr/0001-use-b.md").exists());
    let b = t.read("docs/adr/0002-use-b.md");
    assert!(b.contains("# ADR-0002: b を使う\n"), "{b}");
    assert!(b.contains("[本文](0002-use-b.md) と ADR-0002\n"), "{b}");
    // main から来た行は main の ADR-0001（use-a）を指すので変えない
    assert_eq!(
        t.read("notes.md"),
        "ADR-0002 の続き\n前置き\n1\n2\n3\nADR-0001 は a\n"
    );
    assert!(
        t.read("docs/adr/0001-use-a.md")
            .contains("# ADR-0001: a を使う")
    );
    assert_eq!(t.git(&["rev-parse", "HEAD~1"]), head);
    assert_eq!(
        t.git(&["log", "-1", "--format=%s"]).trim(),
        "docs: renumber ADR-0001 to ADR-0002"
    );
    assert_eq!(t.git(&["status", "--porcelain"]), "");
    assert_eq!(t.tag_message("adr-0002"), "use-b");
    // 振り直した記録の古い予約は解放する（番号 0001 は main の記録が使う）
    assert_eq!(t.git(&["tag", "--list", "adr-0001"]), "");
    let out = t.run(&["check"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert_eq!(stdout(&out), "");
}

#[test]
fn renumber_takes_an_unused_explicit_number() {
    let t = duplicated_after_rebase("explicit");
    t.git(&["tag", "-a", "adr-0005", "-m", "use-e", "main"]);
    // 使った番号（履歴とタグ）には振らない
    for used in ["1", "5"] {
        let out = t.run(&["renumber", "1", used]);
        assert!(!out.status.success(), "{used}: {}", stdout(&out));
    }
    assert!(t.dir.join("docs/adr/0001-use-b.md").exists());
    let out = t.run(&["renumber", "0001", "0007"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "docs/adr/0007-use-b.md\n");
    assert_eq!(t.tag_message("adr-0007"), "use-b");
}

#[test]
fn renumber_refuses_records_on_the_default_branch_and_uncommitted_changes() {
    let t = duplicated_after_rebase("refuse");
    t.write("notes.md", "書きかけ\n");
    let out = t.run(&["renumber", "1"]);
    assert!(!out.status.success(), "{}", stdout(&out));
    t.git(&["checkout", "-q", "--", "notes.md"]);
    t.git(&["switch", "-q", "main"]);
    // main の上では、ADR-0001（use-a）は既定ブランチにある
    let out = t.run(&["renumber", "1"]);
    assert!(!out.status.success(), "{}", stdout(&out));
    assert!(t.dir.join("docs/adr/0001-use-a.md").exists());
    assert_eq!(t.git(&["status", "--porcelain"]), "");
}

#[test]
fn renumber_issue_rewrites_hash_references_and_file_names() {
    let t = TempRepo::new("renumber-issue");
    t.git(&["switch", "-q", "-c", "topic"]);
    t.run(&["new", "--kind", "issue", "fix-b", "b を直す"]);
    t.write(
        "notes.md",
        "#0001 を直す（0001-fix-b.md）。#00012 と x#0001 は別\n",
    );
    t.commit_all("docs: b");
    t.git(&["switch", "-q", "main"]);
    t.write("docs/issues/0001-fix-a.md", "# a を直す\n");
    t.commit_all("docs: a");
    t.git(&["switch", "-q", "topic"]);
    t.git(&["rebase", "-q", "main"]);
    let out = t.run(&["check"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stdout(&out).contains("number 0001 is also used by"),
        "{}",
        stdout(&out)
    );
    let out = t.run(&["renumber", "--kind", "issue", "1"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert_eq!(stdout(&out), "docs/issues/0002-fix-b.md\n");
    assert_eq!(
        t.read("notes.md"),
        "#0002 を直す（0002-fix-b.md）。#00012 と x#0001 は別\n"
    );
    assert_eq!(t.read("docs/issues/0001-fix-a.md"), "# a を直す\n");
    assert_eq!(t.tag_message("issue-0002"), "fix-b");
    assert_eq!(
        t.git(&["log", "-1", "--format=%s"]).trim(),
        "docs: renumber issue #0001 to #0002"
    );
}

#[test]
fn check_warns_when_a_tag_reserves_the_number_for_another_record() {
    let t = TempRepo::new("tag-slug");
    t.run(&["new", "use-a", "a を使う"]);
    t.git(&["add", "-A"]);
    t.git(&["tag", "-a", "adr-0002", "-m", "use-other", "main"]);
    t.write(
        "docs/adr/0002-use-b.md",
        &design_records::template(2, "b を使う", "2026-10-01"),
    );
    t.git(&["add", "-A"]);
    let out = t.run(&["check"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert_eq!(
        stdout(&out),
        "docs/adr/0002-use-b.md: warning: number 0002 is reserved for `use-other` by tag adr-0002; \
         if another record took the number, run `design-records renumber 2`\n"
    );
}

#[test]
fn prune_deletes_tags_of_records_merged_into_the_remote_default_branch() {
    let t = TempRepo::with_remote("prune");
    t.git(&["switch", "-q", "-c", "topic"]);
    t.run(&["new", "use-a", "a を使う"]);
    t.commit_all("docs(adr): a");
    t.git(&["switch", "-q", "-c", "other", "main"]);
    t.run(&["new", "use-b", "b を使う"]);
    t.commit_all("docs(adr): b");
    t.git(&["switch", "-q", "main"]);
    t.git(&["merge", "-q", "--no-ff", "-m", "Merge: a", "topic"]);
    // ローカルでマージしただけで、origin の main にはまだ無い
    let out = t.run(&["prune"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(t.tag_message("adr-0001"), "use-a");
    assert!(t.remote_tag("adr-0001").contains("adr-0001"));
    t.git(&["push", "-q", "origin", "main"]);
    let out = t.run(&["prune"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "adr-0001\n");
    assert_eq!(t.git(&["tag", "--list", "adr-0001"]), "");
    assert_eq!(t.remote_tag("adr-0001"), "");
    // マージしていない記録のタグは残す
    assert_eq!(t.tag_message("adr-0002"), "use-b");
    assert!(t.remote_tag("adr-0002").contains("adr-0002"));
}

#[test]
fn prune_without_a_remote_reads_the_local_default_branch() {
    let t = TempRepo::new("prune-local");
    t.git(&["switch", "-q", "-c", "topic"]);
    t.run(&["new", "--kind", "issue", "fix-a", "a を直す"]);
    t.commit_all("docs: a");
    t.git(&["switch", "-q", "main"]);
    // 番号は履歴にあっても、slug が違う記録のタグは消さない
    t.write("docs/issues/0002-fix-b.md", "# b を直す\n");
    t.commit_all("docs: b");
    t.git(&["tag", "-a", "issue-0002", "-m", "fix-other", "main"]);
    t.git(&["merge", "-q", "--no-ff", "-m", "Merge: a", "topic"]);
    let out = t.run(&["prune"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "issue-0001\n");
    assert_eq!(t.git(&["tag", "--list"]), "issue-0002\n");
}

#[test]
fn new_sees_records_merged_by_other_clones_after_their_tags_are_pruned() {
    let t = TempRepo::with_remote("merged-elsewhere");
    let other = t.clone_remote("other");
    git_in(&other, &["switch", "-q", "-c", "topic"]);
    let out = run(&other, &["new", "use-a", "a を使う"], None);
    assert!(out.status.success(), "{}", stderr(&out));
    git_in(&other, &["add", "-A"]);
    git_in(&other, &["commit", "-q", "-m", "docs(adr): a"]);
    git_in(&other, &["switch", "-q", "main"]);
    git_in(
        &other,
        &["merge", "-q", "--no-ff", "-m", "Merge: a", "topic"],
    );
    git_in(&other, &["push", "-q", "origin", "main"]);
    let out = run(&other, &["prune"], None);
    assert_eq!(stdout(&out), "adr-0001\n", "{}", stderr(&out));
    // この clone は origin/main を fetch していない
    let out = t.run(&["new", "use-b", "b を使う"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "docs/adr/0002-use-b.md\n");
}

#[test]
fn new_follows_the_remote_when_a_stale_local_tag_has_the_same_name() {
    let t = TempRepo::with_remote("stale");
    // push する前に止まった予約が手元に残っている
    t.git(&["tag", "-a", "adr-0001", "-m", "use-stale", "main"]);
    let other = t.clone_remote("other");
    git_in(
        &other,
        &["tag", "-a", "adr-0001", "-m", "use-other", "origin/main"],
    );
    git_in(&other, &["push", "-q", "origin", "refs/tags/adr-0001"]);
    let out = t.run(&["new", "use-b", "b を使う"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "docs/adr/0002-use-b.md\n");
    assert_eq!(t.tag_message("adr-0001"), "use-other");
}

#[test]
fn renumber_reads_paths_with_spaces_and_skips_symlinks() {
    let t = duplicated_after_rebase("paths");
    t.write("plans/a plan.md", "ADR-0001 の計画\n");
    // リポジトリの外のファイルへのリンク
    let outside = t.root.join("outside.md");
    fs::write(&outside, "ADR-0001 は a\n").unwrap();
    std::os::unix::fs::symlink(&outside, t.dir.join("plans/link.md")).unwrap();
    t.commit_all("docs: plan");
    let out = t.run(&["renumber", "1"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert_eq!(t.read("plans/a plan.md"), "ADR-0002 の計画\n");
    // リンクの先は書き換えない（足した行はリンクの blob の行で、先のファイルの行ではない）
    assert_eq!(fs::read_to_string(&outside).unwrap(), "ADR-0001 は a\n");
    assert_eq!(t.git(&["status", "--porcelain"]), "");
}

#[test]
fn renumber_compares_with_the_local_default_branch_when_it_is_ahead_of_origin() {
    let t = TempRepo::with_remote("ahead");
    t.git(&["switch", "-q", "-c", "topic"]);
    t.run(&["new", "use-b", "b を使う"]);
    t.commit_all("docs(adr): b");
    t.git(&["switch", "-q", "main"]);
    // ローカルの main にだけある、機能より前の ADR-0001
    t.write(
        "docs/adr/0001-use-a.md",
        &design_records::template(1, "a を使う", "2026-10-01"),
    );
    t.write("notes.md", "ADR-0001 は a\n");
    t.commit_all("docs(adr): a");
    t.git(&["switch", "-q", "topic"]);
    t.git(&["rebase", "-q", "main"]);
    let out = t.run(&["renumber", "1"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert_eq!(stdout(&out), "docs/adr/0002-use-b.md\n");
    assert_eq!(t.read("notes.md"), "ADR-0001 は a\n");
}

#[test]
fn renumber_restores_the_tree_when_the_commit_is_rejected() {
    let t = duplicated_after_rebase("rejected");
    let hooks = t.root.join("hooks");
    fs::create_dir(&hooks).unwrap();
    let hook = hooks.join("pre-commit");
    fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    t.git(&["config", "core.hooksPath", hooks.to_str().unwrap()]);
    let head = t.git(&["rev-parse", "HEAD"]);
    let out = t.run(&["renumber", "1"]);
    assert!(!out.status.success(), "{}", stdout(&out));
    assert_eq!(t.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(t.git(&["status", "--porcelain"]), "");
    assert!(t.dir.join("docs/adr/0001-use-b.md").exists());
}

#[test]
fn renumber_reads_added_lines_that_look_like_diff_headers() {
    let t = duplicated_after_rebase("plusplus");
    t.git(&["switch", "-q", "main"]);
    t.write("plans/p.md", "a\nb\nc\nd\ne\nf\ng\nh\n");
    t.commit_all("docs: plan");
    t.git(&["switch", "-q", "topic"]);
    t.git(&["rebase", "-q", "main"]);
    // 足した行が `+++ b/...` に見える差分の、後ろの hunk
    t.write(
        "plans/p.md",
        "++ b/other.md\na\nb\nc\nd\ne\nf\ng\nh\nADR-0001 の計画\n",
    );
    t.commit_all("docs: plan b");
    let out = t.run(&["renumber", "1"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert_eq!(
        t.read("plans/p.md"),
        "++ b/other.md\na\nb\nc\nd\ne\nf\ng\nh\nADR-0002 の計画\n"
    );
}

#[test]
fn renumber_rewrites_links_to_three_digit_file_names() {
    let t = TempRepo::new("three-renumber");
    t.git(&["switch", "-q", "-c", "topic"]);
    t.write(
        "docs/adr/005-use-x.md",
        &design_records::template(5, "x を使う", "2026-10-01"),
    );
    t.write("README.md", "[x](docs/adr/005-use-x.md)\n");
    t.commit_all("docs(adr): x");
    let out = t.run(&["renumber", "5", "9"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert_eq!(stdout(&out), "docs/adr/0009-use-x.md\n");
    assert_eq!(t.read("README.md"), "[x](docs/adr/0009-use-x.md)\n");
    assert!(
        t.read("docs/adr/0009-use-x.md")
            .contains("# ADR-0009: x を使う")
    );
}

#[test]
fn renumber_without_a_collision_keeps_the_old_reservation() {
    let t = TempRepo::new("voluntary");
    t.git(&["switch", "-q", "-c", "topic"]);
    t.run(&["new", "use-b", "b を使う"]);
    t.commit_all("docs(adr): b");
    let out = t.run(&["renumber", "1", "7"]);
    assert!(out.status.success(), "{}", stderr(&out));
    // 番号 0001 はほかの記録が使っていないので、予約を残して使い回させない
    assert_eq!(t.tag_message("adr-0001"), "use-b");
}

#[test]
fn new_fetches_before_choosing_where_to_put_the_tag() {
    let t = TempRepo::with_remote("unfetched");
    // origin を足しただけで、まだ何も fetch していない clone
    let fresh = TempRepo::new("unfetched-fresh");
    let bare = t.root.join("remote.git");
    fresh.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
    let out = fresh.run(&["new", "use-a", "a を使う"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(t.remote_tag("adr-0001").contains("adr-0001"));
}

#[test]
fn new_refuses_shallow_clones_whose_history_is_cut() {
    let t = TempRepo::with_remote("shallow");
    t.git(&["commit", "-q", "--allow-empty", "-m", "chore: second"]);
    t.git(&["push", "-q", "origin", "main"]);
    let bare = t.root.join("remote.git");
    git_in(
        &t.root,
        &[
            "clone",
            "-q",
            "--depth",
            "1",
            &format!("file://{}", bare.display()),
            "shallow",
        ],
    );
    let shallow = t.root.join("shallow");
    configure(&shallow);
    // 切られた履歴の先の、消した記録の番号を数えられない
    let out = run(&shallow, &["new", "use-a", "a を使う"], None);
    assert!(!out.status.success(), "{}", stdout(&out));
    assert!(stderr(&out).contains("shallow"), "{}", stderr(&out));
    assert!(!shallow.join("docs").exists());
}

#[test]
fn new_refuses_numbers_beyond_four_digits() {
    let t = TempRepo::new("exhausted");
    t.git(&["tag", "-a", "adr-9999", "-m", "use-last", "main"]);
    // 5 桁の番号は番号として数えないので、振ると使い回しうる
    let out = t.run(&["new", "use-a", "a を使う"]);
    assert!(!out.status.success(), "{}", stdout(&out));
    assert!(!t.dir.join("docs").exists());
}

#[test]
fn check_counts_three_digit_issue_names_as_duplicates() {
    let t = TempRepo::new("issue-three");
    t.write("docs/issues/001-fix-a.md", "# a\n");
    t.write("docs/issues/0001-fix-b.md", "# b\n");
    t.git(&["add", "-A"]);
    let out = t.run(&["check"]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert!(
        stdout(&out).contains("number 0001 is also used by"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn renumber_refuses_untracked_files() {
    let t = duplicated_after_rebase("untracked");
    t.write("draft.md", "書きかけ\n");
    let out = t.run(&["renumber", "1"]);
    assert!(!out.status.success(), "{}", stdout(&out));
    assert!(t.dir.join("docs/adr/0001-use-b.md").exists());
    assert_eq!(t.git(&["tag", "--list", "adr-0002"]), "");
}

#[test]
fn new_drops_local_tags_that_origin_deleted() {
    let t = TempRepo::with_remote("pruned-tag");
    let other = t.clone_remote("other");
    // origin の main に入った ADR-0001（use-a）と、その番号を一度予約して振り直した別の記録（use-b）のタグ
    fs::create_dir_all(other.join("docs/adr")).unwrap();
    fs::write(
        other.join("docs/adr/0001-use-a.md"),
        design_records::template(1, "a を使う", "2026-10-01"),
    )
    .unwrap();
    git_in(&other, &["add", "-A"]);
    git_in(&other, &["commit", "-q", "-m", "docs(adr): a"]);
    git_in(&other, &["push", "-q", "origin", "main"]);
    git_in(
        &other,
        &["tag", "-a", "adr-0001", "-m", "use-b", "origin/main"],
    );
    git_in(&other, &["push", "-q", "origin", "refs/tags/adr-0001"]);
    t.git(&[
        "fetch",
        "-q",
        "origin",
        "refs/tags/adr-0001:refs/tags/adr-0001",
    ]);
    git_in(
        &other,
        &["push", "-q", "origin", "--delete", "refs/tags/adr-0001"],
    );
    let out = t.run(&["new", "use-c", "c を使う"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "docs/adr/0002-use-c.md\n");
    assert_eq!(t.git(&["tag", "--list", "adr-0001"]), "");
}

#[test]
fn renumber_keeps_front_matter_references_of_the_moved_record() {
    let t = duplicated_after_rebase("self-ref");
    let b = "docs/adr/0001-use-b.md";
    // b は main の ADR-0001（use-a）を前提にする。記録の前付けは自分を指さない
    t.write(
        b,
        &t.read(b).replace("requires: []", "requires: [ADR-0001]"),
    );
    t.commit_all("docs(adr): b requires a");
    let out = t.run(&["renumber", "1"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    let b = t.read("docs/adr/0002-use-b.md");
    assert!(b.contains("requires: [ADR-0001]\n"), "{b}");
    assert!(b.contains("# ADR-0002: b を使う\n"), "{b}");
    let out = t.run(&["check"]);
    assert!(out.status.success(), "{}", stdout(&out));
}

#[test]
fn check_does_not_read_date_named_files_as_issues() {
    let t = TempRepo::new("issue-dates");
    t.write("docs/issues/0001-fix-a.md", "# a\n");
    t.write("docs/issues/20261010-notes.md", "# notes\n");
    t.run(&["new", "use-a", "a を使う"]);
    let a = "docs/adr/0001-use-a.md";
    t.write(
        a,
        &t.read(a).replace("issues: []", "issues: [\"#20261010\"]"),
    );
    t.git(&["add", "-A"]);
    let out = t.run(&["check"]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    assert!(
        stdout(&out).contains("`issues` names #20261010, which does not exist in docs/issues"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn new_tells_how_to_publish_the_default_branch_when_origin_has_none() {
    let t = TempRepo::new("empty-origin");
    let bare = t.root.join("remote.git");
    git_in(&t.root, &["init", "-q", "--bare", "remote.git"]);
    t.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
    let out = t.run(&["new", "use-a", "a を使う"]);
    assert!(!out.status.success(), "{}", stdout(&out));
    assert!(stderr(&out).contains("git push"), "{}", stderr(&out));
    assert!(stderr(&out).contains("set-head"), "{}", stderr(&out));
}

#[test]
fn sessions_in_one_clone_do_not_share_a_number_while_one_is_pushing() {
    let t = TempRepo::with_remote("same-clone");
    // 先のセッションの push を遅らせる git
    let bin = t.root.join("bin");
    fs::create_dir(&bin).unwrap();
    let real = String::from_utf8(
        Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let real = real.trim();
    fs::write(
        bin.join("git"),
        format!("#!/bin/sh\ncase \" $* \" in *\" push \"*) sleep 2 ;; esac\nexec {real} \"$@\"\n"),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(bin.join("git"), fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let mut first = Command::new(env!("CARGO_BIN_EXE_design-records"));
    first
        .current_dir(&t.dir)
        .env("PATH", path)
        .args(["new", "use-a", "a を使う"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    isolate(&mut first);
    let first = first.spawn().unwrap();
    // 先のセッションがタグを打ち、push を待っている間に、次のセッションが走る
    std::thread::sleep(std::time::Duration::from_millis(700));
    let second = t.run(&["new", "use-b", "b を使う"]);
    let first = first.wait_with_output().unwrap();
    assert!(first.status.success(), "{}", stderr(&first));
    assert!(second.status.success(), "{}", stderr(&second));
    assert_eq!(stdout(&first), "docs/adr/0001-use-a.md\n");
    assert_eq!(stdout(&second), "docs/adr/0002-use-b.md\n");
    assert!(t.remote_tag("adr-0002").contains("adr-0002"));
}
