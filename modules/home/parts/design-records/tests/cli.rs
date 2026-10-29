//! CLI を一時的な git リポジトリで実行し、テンプレート、検査、status の読み出しを確かめる。

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

    /// git を実行し、成功を確かめて標準出力を返す
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

    /// CLI をリポジトリの `sub`（直下なら ""）で実行する
    fn run_in(&self, sub: &str, args: &[&str]) -> Output {
        run(&self.dir.join(sub), args, None)
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_in("", args)
    }

    /// CLI を、`bin` を PATH の先頭に置いて実行する（git を差し替える）
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

/// `bin/git` に偽の git を書く（`bin` は新しく作る）。`case " $* " in` の腕 `arms` に当たる呼び出しだけを変え、
/// それ以外は本物の git を実行する。腕の中では本物の git を `"$GIT"` で呼ぶ。腕に埋めるパスは [`sh_quote`] で囲む
fn fake_git(bin: &Path, arms: &str) {
    fs::create_dir(bin).unwrap();
    let found = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    let real = String::from_utf8(found.stdout).unwrap();
    assert!(
        found.status.success() && !real.trim().is_empty(),
        "本物の git が PATH に無い"
    );
    let fake = bin.join("git");
    fs::write(
        &fake,
        format!(
            "#!/bin/sh\nGIT={}\ncase \" $* \" in\n{arms}\nesac\nexec \"$GIT\" \"$@\"\n",
            sh_quote(real.trim())
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
}

/// sh の単引用符で囲んだ文字列（偽の git のスクリプトに埋める）
fn sh_quote(s: impl std::fmt::Display) -> String {
    format!("'{}'", s.to_string().replace('\'', "'\\''"))
}

/// git を `dir` で実行し、成功を確かめて標準出力を返す。親の git（hook）から受け継いだ GIT_ で始まる環境変数は外す
/// （外さないと、hook の中で走るテストの操作が、hook を呼んだ本物のリポジトリに効く）。外した後で、テストに要る
/// 設定だけを GIT_CONFIG_COUNT で渡す
fn git_in(dir: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    command
        .current_dir(dir)
        .args(["-c", "core.hooksPath=/dev/null"])
        .args(args);
    isolate_git(&mut command);
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// `command` の中で走る git を、手元の環境の GIT_* から切り離し、全体の設定（nix の sandbox には無い）に頼らない
/// 設定を渡す。init と clone の既定ブランチ（bare のリモートの HEAD）を main にし、clone や init をする場所
/// （リポジトリの外）でも名前を決める
fn isolate_git(command: &mut Command) {
    isolate(command);
    for (i, (key, value)) in [
        ("init.defaultBranch", "main"),
        ("user.name", "t"),
        ("user.email", "t@t"),
    ]
    .into_iter()
    .enumerate()
    {
        command
            .env(format!("GIT_CONFIG_KEY_{i}"), key)
            .env(format!("GIT_CONFIG_VALUE_{i}"), value);
    }
    command.env("GIT_CONFIG_COUNT", "3");
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
    let bin = t.root.join("bin");
    fake_git(&bin, "  *\" log \"*) exit 128 ;;");
    let out = t.run_with_bin(&bin, &["new", "use-j", "j を使う"]);
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
    t.write(
        b,
        &t.read(b).replace("## 背景\n", "## 背景\n\n本文を足す\n"),
    );
    let out = t.run(&["check", b]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert_eq!(
        stdout(&out),
        "docs/adr/0002-use-b.md: warning: the decided sections changed although the ADR is \
         accepted on main; the title, the front matter other than status, superseded-by, date, \
         amends, amended-by, issues, and the context, decision drivers, considered options, \
         decision outcome and pros and cons of the options record the decision as made, so \
         change them only to fix typos, grammar, markup and broken links or to replace words \
         with meaning-preserving ones from the Japanese dictionary (consequences, confirmation, \
         notes and the `## 変更` section may follow the current facts). To change part of the \
         decision, edit the decision outcome and add a dated item to the `## 変更` section that \
         quotes the replaced text and gives the reason; to change all of it, supersede the ADR \
         with a new one\n"
    );
}

#[test]
fn check_lets_consequences_and_changes_follow_a_decided_adr_on_main() {
    let t = superseded_repo("living");
    t.git(&["switch", "-q", "-c", "topic"]);
    let b = "docs/adr/0002-use-b.md";
    t.write(
        b,
        &(t.read(b)
            .replace("### 帰結\n", "### 帰結\n\n- 残りは #9\n")
            .replace("issues: []", "issues: [\"#9\"]")
            + "\n## 変更\n\n- 2026-10-29: 変えた。前は「a」。理由\n"),
    );
    let out = t.run(&["check", b]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert_eq!(stdout(&out), "");
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

/// 一部の置き換えの結びを足したときの違反の文（`path` の ADR の `amends` に ADR-`n` を足した）
fn new_link_error(path: &str, n: u32) -> String {
    format!(
        "{path}: error: `amends` gains ADR-{n:04} since main, but partial changes are no longer \
         linked with amends and amended-by; drop the item and record the change in the ADR whose \
         decision changes: edit its decision outcome and add a dated item to its `## 変更` \
         section that quotes the replaced text and gives the reason\n"
    )
}

#[test]
fn check_rejects_amendment_links_added_since_main() {
    let t = decided_topic("new-links");
    t.run(&["new", "use-b", "b を使う"]);
    let a = "docs/adr/0001-use-a.md";
    let b = "docs/adr/0002-use-b.md";
    t.write(
        a,
        &t.read(a).replace(
            "superseded-by:\n",
            "superseded-by:\namended-by: [ADR-0002]\n",
        ),
    );
    t.write(
        b,
        &t.read(b)
            .replace("status: proposed", "status: accepted")
            .replace("superseded-by:\n", "superseded-by:\namends: [ADR-0001]\n"),
    );
    t.git(&["add", "-A"]);
    let out = t.run(&["check", a, b]);
    assert!(!out.status.success(), "{}", stdout(&out));
    assert_eq!(stdout(&out), new_link_error(b, 1));
}

#[test]
fn check_lets_a_proposed_amends_on_main_be_accepted() {
    let t = TempRepo::new("accept-link");
    t.run(&["new", "use-a", "a を使う"]);
    t.run(&["new", "use-b", "b を使う"]);
    let a = "docs/adr/0001-use-a.md";
    let b = "docs/adr/0002-use-b.md";
    t.write(
        a,
        &t.read(a).replace("status: proposed", "status: accepted"),
    );
    // 決める前の ADR-0002 の amends は、ADR-0001 にまだ amended-by を求めない
    t.write(
        b,
        &t.read(b)
            .replace("superseded-by:\n", "superseded-by:\namends: [ADR-0001]\n"),
    );
    t.commit_all("docs(adr): a and proposed b");
    t.git(&["switch", "-q", "-c", "topic"]);
    t.write(
        a,
        &t.read(a).replace(
            "superseded-by:\n",
            "superseded-by:\namended-by: [ADR-0002]\n",
        ),
    );
    t.write(
        b,
        &t.read(b).replace("status: proposed", "status: accepted"),
    );
    let out = t.run(&["check", a, b]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert_eq!(stdout(&out), "");
}

#[test]
fn check_keeps_amendment_links_already_on_main() {
    let t = TempRepo::new("old-links");
    t.run(&["new", "use-a", "a を使う"]);
    t.run(&["new", "use-b", "b を使う"]);
    let a = "docs/adr/0001-use-a.md";
    let b = "docs/adr/0002-use-b.md";
    t.write(
        a,
        &t.read(a)
            .replace("status: proposed", "status: accepted")
            .replace(
                "superseded-by:\n",
                "superseded-by:\namended-by: [ADR-0002]\n",
            ),
    );
    t.write(
        b,
        &t.read(b)
            .replace("status: proposed", "status: accepted")
            .replace("superseded-by:\n", "superseded-by:\namends: [ADR-0001]\n"),
    );
    t.commit_all("docs(adr): a and b");
    t.git(&["switch", "-q", "-c", "topic"]);
    t.write(
        b,
        &t.read(b).replace("### 帰結\n", "### 帰結\n\n- 残りは #9\n"),
    );
    let out = t.run(&["check", a, b]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert_eq!(stdout(&out), "");
}

#[test]
fn check_follows_renames_when_comparing_the_body() {
    let t = decided_topic("renamed");
    let new = "docs/adr/0001-use-aa.md";
    t.git(&["mv", "docs/adr/0001-use-a.md", new]);
    t.write(
        new,
        &t.read(new).replace("## 背景\n", "## 背景\n\n本文を足す\n"),
    );
    t.git(&["add", "-A"]);
    let out = t.run(&["check", "--changed", new]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(
        stdout(&out).starts_with(&format!("{new}: warning: the decided sections changed")),
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
    t.write(
        c,
        &t.read(c).replace("## 背景\n", "## 背景\n\n本文を足す\n"),
    );
    // 確定した ADR のフロントマターと補足は直してよい
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
    // main の ADR-0002 のフロントマターに空行がある（check の違反）
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
    t.write(
        b,
        &t.read(b).replace("## 背景\n", "## 背景\n\n本文を足す\n"),
    );
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
    // フロントマターのほかの行の形は問わない（docs/issues などの、検査していない文書も読む）。形の検査は check が行う
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
    let mark = t.root.join("raced");
    let bin = t.root.join("bin");
    fake_git(
        &bin,
        &format!(
            "  *\" fetch \"*)\n\
             \x20   \"$GIT\" \"$@\" || exit $?\n\
             \x20   if [ ! -e {mark} ]; then : > {mark}; \"$GIT\" -C {other} push -q origin refs/tags/adr-0001 || exit 99; fi\n\
             \x20   exit 0 ;;",
            mark = sh_quote(mark.display()),
            other = sh_quote(other.display()),
        ),
    );
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
fn new_tags_the_latest_default_branch_when_it_retries() {
    let t = TempRepo::with_remote("retarget");
    let other = t.clone_remote("other");
    git_in(
        &other,
        &["tag", "-a", "adr-0001", "-m", "use-other", "origin/main"],
    );
    // 最初の既定ブランチの fetch の直後に（タグの fetch は済んでいる）、ほかの clone が同じ番号の予約と、main の
    // 新しいコミットを push する git
    let mark = t.root.join("raced");
    let bin = t.root.join("bin");
    fake_git(
        &bin,
        &format!(
            "  *\" fetch \"*refs/heads/*)\n\
             \x20   \"$GIT\" \"$@\" || exit $?\n\
             \x20   if [ ! -e {mark} ]; then : > {mark}; \"$GIT\" -C {other} commit -q --allow-empty -m 'chore: x' && \"$GIT\" -C {other} push -q origin HEAD:main refs/tags/adr-0001 || exit 99; fi\n\
             \x20   exit 0 ;;",
            mark = sh_quote(mark.display()),
            other = sh_quote(other.display()),
        ),
    );
    let out = t.run_with_bin(&bin, &["new", "use-foo", "foo を使う"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(mark.exists());
    assert_eq!(stdout(&out), "docs/adr/0002-use-foo.md\n");
    let latest = git_in(&other, &["rev-parse", "HEAD"]);
    assert_eq!(t.git(&["rev-parse", "adr-0002^{commit}"]), latest);
    // origin の予約のタグ（ほかの clone が見るもの）も、同じ先端を指す
    git_in(
        &other,
        &[
            "fetch",
            "-q",
            "origin",
            "refs/tags/adr-0002:refs/tags/adr-0002",
        ],
    );
    assert_eq!(git_in(&other, &["rev-parse", "adr-0002^{commit}"]), latest);
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
         if another record took the number, run `design-records renumber 2`; \
         if this record is the reserved one and its file name was renamed, \
         run `git tag -f -a adr-0002 -m 'use-b' 'adr-0002^{}'`\n"
    );
}

/// check が示す打ち直しのコマンド（警告の `renamed, run `…`` の中）
fn retag_command_for_origin(t: &TempRepo) -> String {
    let out = t.run(&["check"]);
    let text = stdout(&out);
    let marker = "renamed, run `";
    let start = text
        .find(marker)
        .unwrap_or_else(|| panic!("no retag command: {text}"))
        + marker.len();
    let len = text[start..].find('`').unwrap();
    text[start..start + len].to_string()
}

/// `command` をリポジトリで sh に渡し、終了コードの成否を返す
fn sh_in(t: &TempRepo, command: &str) -> bool {
    let mut sh = Command::new("sh");
    sh.current_dir(&t.dir).args(["-c", command]);
    isolate_git(&mut sh);
    sh.output().unwrap().status.success()
}

/// origin に `adr-0002` を `use-other` で予約し、改名した記録 `0002-use-b` を置いたリポジトリ
fn renamed_reserved_record_with_remote(name: &str) -> TempRepo {
    let t = TempRepo::with_remote(name);
    t.run(&["new", "use-a", "a を使う"]);
    t.git(&["add", "-A"]);
    t.git(&["tag", "-a", "adr-0002", "-m", "use-other", "main"]);
    t.git(&["push", "-q", "origin", "refs/tags/adr-0002"]);
    t.write(
        "docs/adr/0002-use-b.md",
        &design_records::template(2, "b を使う", "2026-10-01"),
    );
    t.git(&["add", "-A"]);
    t
}

#[test]
fn retag_command_for_origin_moves_the_reservation_on_origin() {
    let t = renamed_reserved_record_with_remote("retag-origin");
    let command = retag_command_for_origin(&t);
    assert!(sh_in(&t, &command), "{command}");
    assert_eq!(t.tag_message("adr-0002"), "use-b");
    let local = t.git(&["rev-parse", "refs/tags/adr-0002"]);
    assert!(t.remote_tag("adr-0002").starts_with(local.trim()));
}

#[test]
fn retag_command_for_origin_keeps_a_reservation_another_clone_moved() {
    let t = renamed_reserved_record_with_remote("retag-origin-moved");
    let other = t.clone_remote("other");
    git_in(
        &other,
        &["tag", "-f", "-a", "adr-0002", "-m", "use-c", "adr-0002^{}"],
    );
    git_in(
        &other,
        &["push", "-q", "--force", "origin", "refs/tags/adr-0002"],
    );
    let moved = git_in(&other, &["rev-parse", "refs/tags/adr-0002"]);
    let before = t.git(&["rev-parse", "refs/tags/adr-0002"]);
    let command = retag_command_for_origin(&t);
    assert!(!sh_in(&t, &command), "{command}");
    assert!(t.remote_tag("adr-0002").starts_with(moved.trim()));
    // 手元のタグは打つ前に戻り、check は予約の食い違いを示し続ける
    assert_eq!(t.git(&["rev-parse", "refs/tags/adr-0002"]), before);
    assert_eq!(retag_command_for_origin(&t), command);
}

#[test]
fn check_omits_the_retag_fix_when_the_reserved_record_exists_too() {
    let t = TempRepo::new("tag-slug-both");
    t.run(&["new", "use-a", "a を使う"]);
    t.git(&["add", "-A"]);
    t.git(&["tag", "-a", "adr-0002", "-m", "use-other", "main"]);
    t.write(
        "docs/adr/0002-use-other.md",
        &design_records::template(2, "other を使う", "2026-10-01"),
    );
    t.write(
        "docs/adr/0002-use-b.md",
        &design_records::template(2, "b を使う", "2026-10-01"),
    );
    t.git(&["add", "-A"]);
    let out = t.run(&["check"]);
    let warning = stdout(&out)
        .lines()
        .find(|l| l.starts_with("docs/adr/0002-use-b.md: warning"))
        .unwrap_or_default()
        .to_string();
    assert!(warning.contains("renumber 2"), "{}", stdout(&out));
    assert!(!warning.contains("git tag"), "{}", stdout(&out));
}

#[test]
fn check_quotes_the_slug_in_the_retag_command() {
    let t = TempRepo::new("tag-slug-quote");
    t.run(&["new", "use-a", "a を使う"]);
    t.git(&["add", "-A"]);
    t.git(&["tag", "-a", "adr-0002", "-m", "use-other", "main"]);
    t.write(
        "docs/adr/0002-use b&c.md",
        &design_records::template(2, "b を使う", "2026-10-01"),
    );
    t.git(&["add", "-A"]);
    let out = t.run(&["check"]);
    assert!(stdout(&out).contains("-m 'use b&c' "), "{}", stdout(&out));
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
    // 旧番号の記録（use-a）は origin にまだ無いので、ほかの clone からは番号が空いて見える。予約を残す
    assert_eq!(t.tag_message("adr-0001"), "use-b");
    assert!(t.remote_tag("adr-0001").contains("adr-0001"));
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
    // 番号 0001 はほかの記録が使っていないので、予約を残して再利用させない
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
    // 5 桁の番号は番号として数えないので、振ると再利用しうる
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
    // b は main の ADR-0001（use-a）を前提にする。記録のフロントマターは自分を指さない
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
    // 先のセッションの push を、入ったことをファイルで知らせてから遅らせる git
    let pushing = t.root.join("pushing");
    let bin = t.root.join("bin");
    fake_git(
        &bin,
        &format!(
            "  *\" push \"*) : > {}; sleep 2 ;;",
            sh_quote(pushing.display())
        ),
    );
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
    // 先のセッションがタグを打って push に入ってから、次のセッションを実行する
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !pushing.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "the first session never pushed"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let second = t.run(&["new", "use-b", "b を使う"]);
    let first = first.wait_with_output().unwrap();
    assert!(first.status.success(), "{}", stderr(&first));
    assert!(second.status.success(), "{}", stderr(&second));
    assert_ne!(stdout(&first), stdout(&second));
    assert!(t.remote_tag("adr-0001").contains("adr-0001"));
    assert!(t.remote_tag("adr-0002").contains("adr-0002"));
}

#[test]
fn renumber_ignores_context_lines_inside_merged_hunks() {
    let t = duplicated_after_rebase("inter-hunk");
    t.git(&["switch", "-q", "main"]);
    t.write("plans/p.md", "x\nADR-0001 は a\ny\n");
    t.commit_all("docs: plan");
    t.git(&["switch", "-q", "topic"]);
    t.git(&["rebase", "-q", "main"]);
    t.write(
        "plans/p.md",
        "ADR-0001 の続き\nx\nADR-0001 は a\ny\nADR-0001 の後\n",
    );
    t.commit_all("docs: plan b");
    // 近い hunk を 1 つにまとめる設定では、間の main の行が hunk の範囲に入る
    t.git(&["config", "diff.interHunkContext", "5"]);
    let out = t.run(&["renumber", "1"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert_eq!(
        t.read("plans/p.md"),
        "ADR-0002 の続き\nx\nADR-0001 は a\ny\nADR-0002 の後\n"
    );
}

#[test]
fn new_keeps_remote_tracking_branches_that_origin_lacks() {
    let t = TempRepo::with_remote("keep-branches");
    let head = t.git(&["rev-parse", "HEAD"]).trim().to_string();
    // ほかの refspec（PR など）で取った、origin のブランチには無い ref
    t.git(&["update-ref", "refs/remotes/origin/pr/7", &head]);
    let out = t.run(&["new", "use-a", "a を使う"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        t.git(&["rev-parse", "refs/remotes/origin/pr/7"]).trim(),
        head
    );
}

#[test]
fn prune_treats_tags_already_deleted_on_origin_as_released() {
    let t = TempRepo::with_remote("prune-gone");
    t.git(&["switch", "-q", "-c", "topic"]);
    t.run(&["new", "use-a", "a を使う"]);
    t.commit_all("docs(adr): a");
    t.git(&["switch", "-q", "main"]);
    t.git(&["merge", "-q", "--no-ff", "-m", "Merge: a", "topic"]);
    t.git(&["push", "-q", "origin", "main"]);
    // タグの一覧の最初の ls-remote の直後に、ほかの clone がタグを消す git
    let mark = t.root.join("listed");
    let bin = t.root.join("bin");
    fake_git(
        &bin,
        &format!(
            "  *\" ls-remote --tags \"*)\n\
             \x20   if [ ! -e {mark} ]; then : > {mark}; \"$GIT\" \"$@\"; \"$GIT\" -C {dir} push -q origin --delete refs/tags/adr-0001; exit 0; fi ;;",
            mark = sh_quote(mark.display()),
            dir = sh_quote(t.dir.display()),
        ),
    );
    let out = t.run_with_bin(&bin, &["prune"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(mark.exists());
    assert_eq!(stdout(&out), "adr-0001\n");
    assert_eq!(t.git(&["tag", "--list"]), "");
}

#[test]
fn check_matches_reservations_of_three_digit_issues() {
    let t = TempRepo::new("issue-three-tag");
    t.write("docs/issues/001-fix-a.md", "# a\n");
    t.git(&["add", "-A"]);
    t.git(&["tag", "-a", "issue-0001", "-m", "fix-other", "main"]);
    let out = t.run(&["check"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(
        stdout(&out)
            .contains("docs/issues/001-fix-a.md: warning: number 0001 is reserved for `fix-other`"),
        "{}",
        stdout(&out)
    );
}

/// 振り直しの対象の重複と、submodule（sub）を持つリポジトリ
fn duplicated_with_submodule(name: &str) -> TempRepo {
    let t = duplicated_after_rebase(name);
    let sub = TempRepo::new(&format!("{name}-inner"));
    sub.write("x.txt", "x\n");
    sub.commit_all("chore: x");
    sub.write("x.txt", "y\n");
    sub.commit_all("chore: y");
    t.git(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        "-q",
        sub.dir.to_str().unwrap(),
        "sub",
    ]);
    t.commit_all("chore: sub");
    t
}

#[test]
fn renumber_ignores_uncommitted_changes_inside_submodules() {
    let t = duplicated_with_submodule("submodule-dirty");
    fs::write(t.dir.join("sub/x.txt"), "changed\n").unwrap();
    let out = t.run(&["renumber", "1"]);
    assert!(
        out.status.success(),
        "{}{}",
        stdout(&out),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!t.dir.join("docs/adr/0001-use-b.md").exists());
    assert!(
        !t.git(&["show", "--stat", "--format=", "HEAD"])
            .contains("sub")
    );
    assert_eq!(
        fs::read_to_string(t.dir.join("sub/x.txt")).unwrap(),
        "changed\n"
    );
}

#[test]
fn renumber_refuses_a_moved_submodule_commit() {
    let t = duplicated_with_submodule("submodule-moved");
    git_in(&t.dir.join("sub"), &["checkout", "-q", "HEAD~1"]);
    let out = t.run(&["renumber", "1"]);
    assert!(!out.status.success(), "{}", stdout(&out));
    assert!(t.dir.join("docs/adr/0001-use-b.md").exists());
}

#[test]
fn renumber_sees_untracked_files_even_when_status_hides_them() {
    let t = duplicated_after_rebase("hidden-untracked");
    t.git(&["config", "status.showUntrackedFiles", "no"]);
    t.write("draft.md", "書きかけ\n");
    let out = t.run(&["renumber", "1"]);
    assert!(!out.status.success(), "{}", stdout(&out));
    assert!(t.dir.join("docs/adr/0001-use-b.md").exists());
}

#[test]
fn prune_falls_back_when_origin_head_points_nowhere() {
    let t = TempRepo::with_remote("dangling-head");
    t.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/gone",
    ]);
    let out = t.run(&["prune"]);
    assert!(out.status.success(), "{}", stderr(&out));
}

#[test]
fn dash_c_runs_in_another_worktree() {
    let t = duplicated_after_rebase("dash-c");
    let elsewhere = std::env::temp_dir();
    let dir = t.dir.to_str().unwrap();
    let out = run(&elsewhere, &["-C", dir, "check"], None);
    assert_eq!(
        out.status.code(),
        Some(1),
        "{}{}",
        stdout(&out),
        stderr(&out)
    );
    assert!(
        stdout(&out).contains("number 0001 is also used by"),
        "{}",
        stdout(&out)
    );
    let out = run(&elsewhere, &["-C", dir, "renumber", "1"], None);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "docs/adr/0002-use-b.md\n");
    let out = run(
        &elsewhere,
        &["-C", "/nonexistent-design-records", "check"],
        None,
    );
    assert!(!out.status.success());
}

#[test]
fn new_counts_a_record_merged_and_pruned_between_its_fetches() {
    let t = TempRepo::with_remote("merge-between");
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
    // この clone がブランチを取ってきた直後に、ほかの clone がマージを push して予約を解放する git
    let mark = t.root.join("merged");
    let bin = t.root.join("bin");
    fake_git(
        &bin,
        &format!(
            "  *\" fetch \"*\"+refs/heads/\"*)\n\
             \x20   \"$GIT\" \"$@\" || exit $?\n\
             \x20   if [ ! -e {mark} ]; then : > {mark}; \"$GIT\" -C {other} push -q origin main && \"$GIT\" -C {other} push -q origin --delete refs/tags/adr-0001 || exit 99; fi\n\
             \x20   exit 0 ;;",
            mark = sh_quote(mark.display()),
            other = sh_quote(other.display()),
        ),
    );
    let out = t.run_with_bin(&bin, &["new", "use-b", "b を使う"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(mark.exists());
    assert_eq!(stdout(&out), "docs/adr/0002-use-b.md\n");
}

#[test]
fn new_falls_back_to_main_when_the_remote_head_dangles() {
    let t = TempRepo::with_remote("remote-head");
    git_in(
        &t.root.join("remote.git"),
        &["symbolic-ref", "HEAD", "refs/heads/gone"],
    );
    // origin を足しただけで、まだ何も fetch していない clone
    let fresh = TempRepo::new("remote-head-fresh");
    let bare = t.root.join("remote.git");
    fresh.git(&["remote", "add", "origin", bare.to_str().unwrap()]);
    let out = fresh.run(&["new", "use-a", "a を使う"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let main = t.git(&["rev-parse", "main"]).trim().to_string();
    assert_eq!(tag_target(&fresh.dir, "adr-0001"), main);
}

#[test]
fn renumber_skips_submodule_entries() {
    let t = duplicated_after_rebase("gitlink");
    let head = t.git(&["rev-parse", "HEAD"]).trim().to_string();
    t.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{head},vendor/sub"),
    ]);
    fs::create_dir_all(t.dir.join("vendor/sub")).unwrap();
    t.git(&["commit", "-q", "-m", "chore: submodule"]);
    let out = t.run(&["renumber", "1"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert_eq!(stdout(&out), "docs/adr/0002-use-b.md\n");
}

#[test]
fn new_stops_when_origin_has_no_default_branch_despite_a_stale_tracking_ref() {
    let t = TempRepo::with_remote("stale-tracking");
    t.git(&["push", "-q", "origin", "main:trunk"]);
    git_in(
        &t.root.join("remote.git"),
        &["update-ref", "-d", "refs/heads/main"],
    );
    // 手元には、origin で消えた main の古い追跡の ref が残っている
    assert!(t.git(&["rev-parse", "origin/main"]).trim().len() == 40);
    let out = t.run(&["new", "use-a", "a を使う"]);
    assert!(!out.status.success(), "{}", stdout(&out));
    assert!(!t.dir.join("docs").exists());
}

#[test]
fn prune_reads_the_branch_that_origin_names_as_its_head() {
    let t = TempRepo::with_remote("stale-head");
    let first = t.git(&["rev-parse", "HEAD"]).trim().to_string();
    // 手元の origin/HEAD は、古い追跡の ref を指している
    t.git(&["update-ref", "refs/remotes/origin/old", &first]);
    t.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/old",
    ]);
    t.git(&["switch", "-q", "-c", "topic"]);
    t.run(&["new", "use-a", "a を使う"]);
    t.commit_all("docs(adr): a");
    t.git(&["switch", "-q", "main"]);
    t.git(&["merge", "-q", "--no-ff", "-m", "Merge: a", "topic"]);
    t.git(&["push", "-q", "origin", "main"]);
    let out = t.run(&["prune"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "adr-0001\n");
}

#[test]
fn renumber_counts_blank_context_lines_printed_without_a_space() {
    let t = duplicated_after_rebase("blank-context");
    t.git(&["switch", "-q", "main"]);
    t.write("plans/p.md", "x\n\nADR-0001 は a\ny\n");
    t.commit_all("docs: plan");
    t.git(&["switch", "-q", "topic"]);
    t.git(&["rebase", "-q", "main"]);
    t.write(
        "plans/p.md",
        "ADR-0001 の続き\nx\n\nADR-0001 は a\ny\nADR-0001 の後\n",
    );
    t.commit_all("docs: plan b");
    t.git(&["config", "diff.interHunkContext", "5"]);
    t.git(&["config", "diff.suppressBlankEmpty", "true"]);
    let out = t.run(&["renumber", "1"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert_eq!(
        t.read("plans/p.md"),
        "ADR-0002 の続き\nx\n\nADR-0001 は a\ny\nADR-0002 の後\n"
    );
}

#[test]
fn renumber_works_on_a_branch_without_a_common_ancestor() {
    let t = TempRepo::new("no-ancestor");
    t.git(&["checkout", "-q", "--orphan", "lone"]);
    t.run(&["new", "use-a", "a を使う"]);
    t.write("notes.md", "ADR-0001 を見る\n");
    t.commit_all("docs(adr): a");
    let out = t.run(&["renumber", "1", "5"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert_eq!(stdout(&out), "docs/adr/0005-use-a.md\n");
    assert_eq!(t.read("notes.md"), "ADR-0005 を見る\n");
}

#[test]
fn renumber_without_a_common_ancestor_finds_references_and_file_links() {
    let t = TempRepo::new("no-ancestor-grep");
    t.git(&["checkout", "-q", "--orphan", "lone"]);
    t.run(&["new", "use-a", "a を使う"]);
    t.write("notes.md", "ADR-0001 を見る\n");
    t.write("links.md", "[a](docs/adr/0001-use-a.md)\n");
    t.commit_all("docs(adr): a");
    // 色の設定があっても、絞り込みのファイル名を読み違えない
    t.git(&["config", "color.ui", "always"]);
    let out = t.run(&["renumber", "1", "5"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert_eq!(t.read("notes.md"), "ADR-0005 を見る\n");
    assert_eq!(t.read("links.md"), "[a](docs/adr/0005-use-a.md)\n");
}

#[test]
fn renumber_without_a_common_ancestor_stops_when_a_mentioning_file_cannot_be_read() {
    let t = TempRepo::new("no-ancestor-unreadable");
    t.git(&["checkout", "-q", "--orphan", "lone"]);
    t.run(&["new", "use-a", "a を使う"]);
    t.write("notes.md", "ADR-0001 を見る\n");
    t.commit_all("docs(adr): a");
    t.git(&["config", "core.fileMode", "false"]);
    t.git(&["config", "core.trustctime", "false"]);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    t.git(&["update-index", "-q", "--really-refresh"]);
    use std::os::unix::fs::PermissionsExt;
    let notes = t.dir.join("notes.md");
    fs::set_permissions(&notes, fs::Permissions::from_mode(0o000)).unwrap();
    let out = t.run(&["renumber", "1", "5"]);
    fs::set_permissions(&notes, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(!out.status.success(), "{}", stdout(&out));
    assert!(t.dir.join("docs/adr/0001-use-a.md").exists());
}

#[test]
fn retries_see_records_merged_and_pruned_after_the_push_is_rejected() {
    let t = TempRepo::with_remote("retry-merge");
    let other = t.clone_remote("other");
    // ほかの clone が ADR-0001（use-a）を作ってマージしてあり、予約のタグはまだ push していない
    git_in(&other, &["switch", "-q", "-c", "topic"]);
    fs::create_dir_all(other.join("docs/adr")).unwrap();
    fs::write(
        other.join("docs/adr/0001-use-a.md"),
        design_records::template(1, "a を使う", "2026-10-01"),
    )
    .unwrap();
    git_in(&other, &["add", "-A"]);
    git_in(&other, &["commit", "-q", "-m", "docs(adr): a"]);
    git_in(&other, &["switch", "-q", "main"]);
    git_in(
        &other,
        &["merge", "-q", "--no-ff", "-m", "Merge: a", "topic"],
    );
    git_in(
        &other,
        &["tag", "-a", "adr-0001", "-m", "use-a", "origin/main"],
    );
    // 最初のタグの fetch の直後にほかの clone が予約し、push が拒まれた後の確認の直後に、マージを push して
    // 予約を解放する git
    let (reserved, merged) = (t.root.join("reserved"), t.root.join("merged"));
    let bin = t.root.join("bin");
    fake_git(
        &bin,
        &format!(
            "  *\" fetch \"*\"refs/tags/\"*)\n\
             \x20   \"$GIT\" \"$@\" || exit $?\n\
             \x20   if [ ! -e {reserved} ]; then : > {reserved}; \"$GIT\" -C {other} push -q origin refs/tags/adr-0001 || exit 99; fi\n\
             \x20   exit 0 ;;\n\
             \x20 *\" ls-remote --tags \"*)\n\
             \x20   \"$GIT\" \"$@\" || exit $?\n\
             \x20   if [ ! -e {merged} ]; then : > {merged}; \"$GIT\" -C {other} push -q origin main && \"$GIT\" -C {other} push -q origin --delete refs/tags/adr-0001 || exit 99; fi\n\
             \x20   exit 0 ;;",
            reserved = sh_quote(reserved.display()),
            merged = sh_quote(merged.display()),
            other = sh_quote(other.display()),
        ),
    );
    let out = t.run_with_bin(&bin, &["new", "use-b", "b を使う"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(merged.exists());
    assert_eq!(stdout(&out), "docs/adr/0002-use-b.md\n");
}
