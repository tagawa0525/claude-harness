//! CLI を一時的な git リポジトリで走らせ、雛形、検査、status の読み出しを確かめる。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use std::io::Write;

/// テストで作る一時的なリポジトリ。落ちたときも含めて、片付けで消す。
struct TempRepo {
    dir: PathBuf,
}

impl TempRepo {
    /// プロセスと時刻とテストの名前で一意にした、main ブランチの空のリポジトリ
    fn new(name: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "design-records-{name}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir(&dir).unwrap();
        let repo = TempRepo { dir };
        repo.git(&["init", "-q", "-b", "main"]);
        repo
    }

    /// git を走らせ、成功を確かめて標準出力を返す。親の git（hook）から受け継いだ GIT_ で始まる環境変数は外す
    /// （外さないと、hook の中で走るテストの操作が、hook を呼んだ本物のリポジトリに効く）
    fn git(&self, args: &[&str]) -> String {
        let mut command = Command::new("git");
        command
            .current_dir(&self.dir)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
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
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        // 片付けに失敗しても、テストの結果は変えない（一時的なディレクトリが残るだけ）
        let _ = fs::remove_dir_all(&self.dir);
    }
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
