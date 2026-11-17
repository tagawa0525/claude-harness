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
    #[cfg(unix)]
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
///
/// Unix だけ: 偽の git は sh のスクリプトで、Windows の `Command` は PATH から `.exe` しか探さず、偽物を呼べない。
/// これを使うテストが起こす git の失敗の分岐は OS によらないので、Linux で確かめる（ADR-0083）
#[cfg(unix)]
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
#[cfg(unix)]
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
    let config = [
        ("init.defaultBranch", "main"),
        ("user.name", "t"),
        ("user.email", "t@t"),
        // clone の checkout は configure より先に走るので、ここでも改行の変換を止める（configure の注記）
        ("core.autocrlf", "false"),
    ];
    for (i, (key, value)) in config.iter().enumerate() {
        command
            .env(format!("GIT_CONFIG_KEY_{i}"), key)
            .env(format!("GIT_CONFIG_VALUE_{i}"), value);
    }
    command.env("GIT_CONFIG_COUNT", config.len().to_string());
}

/// CLI が作るコミットとタグのための、リポジトリの設定（署名と hook は使わない）。改行の変換は止め、期待値の
/// LF をそのまま比べる（Git for Windows の既定はシステムの設定の core.autocrlf=true で、checkout が CRLF にする）
fn configure(dir: &Path) {
    for (key, value) in [
        ("core.autocrlf", "false"),
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
fn new_names_the_verb_first_rule_when_an_adr_slug_is_rejected() {
    let t = TempRepo::new("bad-verb");
    let out = t.run(&["new", "Foo_Usage", "foo を使う"]);
    assert!(!out.status.success());
    assert!(
        stderr(&out).contains("start it with the verb of the decision"),
        "{}",
        stderr(&out)
    );
    // issue の slug は決定ではないので、動詞の規則を添えない
    let out = t.run(&["new", "--kind", "issue", "Bad_Slug", "壊れる"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("kebab-case"), "{}", stderr(&out));
    assert!(!stderr(&out).contains("verb"), "{}", stderr(&out));
}

#[cfg(unix)]
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

#[test]
fn check_warns_about_the_format_of_issues_with_front_matter() {
    let t = TempRepo::new("issue-format");
    let good = "---\nstatus: closed\nresolution: done\n---\n\n# ok\n\n## 問題\n\nx\n\n## 完了の条件\n\n- [x] y\n";
    t.write("docs/issues/0001-good.md", good);
    t.write(
        "docs/issues/0002-bad.md",
        "---\nstatus: closed\n---\n\n# bad\n\n## 問題\n\nx\n",
    );
    t.write(
        "docs/issues/0003-state.md",
        "---\nstatus: wip\n---\n\n## 問題\n\n## 完了の条件\n\n- [ ] y\n",
    );
    // フロントマターの無い issue は、別の形で書くリポジトリのものとして見ない
    t.write("docs/issues/0004-plain.md", "# plain\n");
    t.git(&["add", "-A"]);
    let out = t.run(&["check"]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    assert_eq!(
        stdout(&out),
        "docs/issues/0002-bad.md: warning: a closed issue needs `resolution` \
         (done | wontfix | by-design | external | cannot-reproduce | duplicate | obsolete)\n\
         docs/issues/0002-bad.md: warning: no section `完了の条件`\n\
         docs/issues/0003-state.md: warning: `status` must be open or closed\n"
    );
}

#[test]
fn check_warns_when_the_conditions_section_has_no_checklist() {
    let t = TempRepo::new("issue-checklist");
    t.write(
        "docs/issues/0001-prose.md",
        "---\nstatus: open\n---\n\n## 問題\n\nx\n\n## 完了の条件\n\n直ったら終わり\n",
    );
    t.write(
        "docs/issues/0002-empty.md",
        "---\nstatus: open\n---\n\n## 問題\n\nx\n\n## 完了の条件\n",
    );
    t.git(&["add", "-A"]);
    let out = t.run(&["check"]);
    assert_eq!(
        stdout(&out),
        "docs/issues/0001-prose.md: warning: section `完了の条件` has no checklist item (`- [ ]` or `- [x]`)\n\
         docs/issues/0002-empty.md: warning: section `完了の条件` has no checklist item (`- [ ]` or `- [x]`)\n"
    );
}

/// 問題と完了の条件の節を持つ issue（フロントマターの行は `front`）
fn issue(front: &str) -> String {
    format!("---\n{front}\n---\n\n# x\n\n## 問題\n\nx\n\n## 完了の条件\n\n- [ ] y\n")
}

#[test]
fn check_warns_about_comments_on_issue_front_matter_lines() {
    let t = TempRepo::new("issue-comment");
    t.write(
        "docs/issues/0001-commented.md",
        &issue("status: closed\nresolution: done\nopened: 2026-10-10 # 気づいた日"),
    );
    // 引用符で囲んだ番号は値。引用符なしの値と一覧の要素の先頭の # も、YAML ではコメント（か読めない一覧）になる
    t.write(
        "docs/issues/0002-numbers.md",
        &issue("status: closed\nresolution: duplicate\nduplicate-of: \"#0001\"\nblocked-by: [\"#0001\"]"),
    );
    t.write(
        "docs/issues/0003-bare.md",
        &issue("status: closed\nresolution: duplicate\nduplicate-of: #0001\nblocked-by: [#0001]"),
    );
    t.git(&["add", "-A"]);
    let out = t.run(&["check"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(
        stdout(&out).contains(
            "docs/issues/0001-commented.md:4: warning: the value of `opened` has `#` outside \
             quotes, which YAML reads as a comment but design-records reads as part of the value; \
             write only the value on the key line and quote issue numbers (\"#0012\")\n"
        ),
        "{}",
        stdout(&out)
    );
    for line in [4, 5] {
        assert!(
            stdout(&out).contains(&format!(
                "docs/issues/0003-bare.md:{line}: warning: the value of"
            )),
            "{}",
            stdout(&out)
        );
    }
    assert!(!stdout(&out).contains("0002-numbers"), "{}", stdout(&out));
}

#[test]
fn check_finds_link_targets_that_are_tracked_but_deleted_in_the_working_tree() {
    let t = TempRepo::new("issue-links-deleted");
    t.write(
        "docs/issues/0003-gone.md",
        &issue("status: closed\nresolution: done"),
    );
    t.write(
        "docs/issues/0005-after.md",
        &issue("status: open\nblocked-by: [\"#0003\"]"),
    );
    t.commit_all("docs(issues): 3 and 5");
    fs::remove_file(t.dir.join("docs/issues/0003-gone.md")).unwrap();
    let out = t.run(&["check"]);
    assert!(!stdout(&out).contains("does not exist"), "{}", stdout(&out));
}

#[test]
fn check_warns_about_inconsistent_issue_links() {
    let t = TempRepo::new("issue-links");
    t.run(&["new", "use-a", "a を使う"]);
    t.write(
        "docs/issues/0001-early.md",
        &issue("status: open\nresolution: done\nclosed: 2026-10-11"),
    );
    t.write(
        "docs/issues/0002-dup.md",
        &issue("status: closed\nresolution: duplicate"),
    );
    t.write(
        "docs/issues/0003-blocked.md",
        &issue(
            "status: closed\nresolution: done\nresolved-by: 0001-use-a\nblocked-by: [\"#0001\"]",
        ),
    );
    t.write(
        "docs/issues/0004-missing.md",
        &issue("status: closed\nresolution: duplicate\nduplicate-of: \"#0009\"\nresolved-by: 0007-use-g\nblocked-by: [\"#0008\"]"),
    );
    // 閉じた相手に塞がれていた issue と、コミットの hash で解決した issue は黙る
    t.write(
        "docs/issues/0005-fine.md",
        &issue("status: closed\nresolution: done\nresolved-by: 1a2b3c4\nblocked-by: [\"#0003\"]"),
    );
    t.git(&["add", "-A"]);
    let out = t.run(&["check"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert_eq!(
        stdout(&out),
        "docs/issues/0001-early.md: warning: an open issue has `resolution`, `closed`; write \
         them when closing it\n\
         docs/issues/0002-dup.md: warning: a duplicate issue needs `duplicate-of`\n\
         docs/issues/0003-blocked.md: warning: the issue is closed but `blocked-by` names #0001, \
         which is open; close it after the issues blocking it are closed\n\
         docs/issues/0004-missing.md: warning: `blocked-by` names #0008, which does not exist in \
         docs/issues\n\
         docs/issues/0004-missing.md: warning: `duplicate-of` names #0009, which does not exist in \
         docs/issues\n\
         docs/issues/0004-missing.md: warning: `resolved-by` names 0007-use-g, which is not an \
         ADR in docs/adr\n"
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
         accepted on main; the title, the front matter other than type, status, superseded-by, date, \
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

/// main で ADR-0001 を ADR-0003（accepted）と ADR-0004（rejected）が requires に持ち、topic のブランチで ADR-0002 が
/// ADR-0001 を置き換える
fn superseding_topic(name: &str) -> TempRepo {
    let t = TempRepo::new(name);
    for slug in ["use-a", "use-c", "use-d"] {
        t.run(&["new", slug, "x"]);
    }
    let a = "docs/adr/0001-use-a.md";
    t.write(
        a,
        &t.read(a).replace("status: proposed", "status: accepted"),
    );
    for (path, status) in [
        ("docs/adr/0002-use-c.md", "accepted"),
        ("docs/adr/0003-use-d.md", "rejected"),
    ] {
        t.write(
            path,
            &t.read(path)
                .replace("status: proposed", &format!("status: {status}"))
                .replace("requires: []", "requires: [ADR-0001]"),
        );
    }
    t.commit_all("docs(adr): a, c and d");
    t.git(&["switch", "-q", "-c", "topic"]);
    t.run(&["new", "use-b", "b を使う"]);
    let b = "docs/adr/0004-use-b.md";
    t.write(
        b,
        &t.read(b)
            .replace("status: proposed", "status: accepted")
            .replace("supersedes: []", "supersedes: [ADR-0001]")
            .replace("requires: []", "requires: [ADR-0001]"),
    );
    t.write(
        a,
        &t.read(a)
            .replace("status: accepted", "status: superseded")
            .replace("superseded-by:", "superseded-by: ADR-0004"),
    );
    // check は追跡している ADR だけを読む
    t.git(&["add", "-A"]);
    t
}

#[test]
fn check_names_the_adrs_that_require_an_adr_superseded_on_the_branch() {
    let t = superseding_topic("requires-superseded");
    let out = t.run(&["check", "docs/adr/0001-use-a.md", "docs/adr/0004-use-b.md"]);
    assert!(out.status.success(), "{}", stdout(&out));
    // 置き換えた ADR-0004 と、効力を持たなかった ADR-0003 は見直す相手に挙げない
    assert!(
        stdout(&out).contains(
            "docs/adr/0001-use-a.md: warning: the ADR became superseded since main; review \
             whether the decisions of the ADRs that still list it in `requires` hold with \
             ADR-0004, which supersedes it: ADR-0002\n"
        ),
        "{}",
        stdout(&out)
    );
    // main で既に置き換えられていた ADR には出さない（一度見直した。requires は確定した後に直せない）
    t.commit_all("docs(adr): supersede a");
    t.git(&["switch", "-q", "main"]);
    t.git(&["merge", "-q", "--no-ff", "-m", "Merge", "topic"]);
    t.git(&["switch", "-q", "-c", "later"]);
    let a = "docs/adr/0001-use-a.md";
    t.write(
        a,
        &(t.read(a) + "\n## 変更\n\n- 2026-11-01: ADR-0004 で置き換えた\n"),
    );
    t.git(&["add", "-A"]);
    let out = t.run(&["check", a]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(
        !stdout(&out).contains("became superseded"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn check_names_the_dependents_of_a_superseded_adr_without_a_successor() {
    let t = superseding_topic("requires-no-successor");
    let a = "docs/adr/0001-use-a.md";
    t.write(
        a,
        &t.read(a)
            .replace("superseded-by: ADR-0004", "superseded-by:"),
    );
    t.git(&["add", "-A"]);
    let out = t.run(&["check", a]);
    // superseded-by が空なのは違反（別に示す）。注意は置き換えた ADR を挙げずに、見直す相手を示す
    assert!(
        stdout(&out).contains(
            "docs/adr/0001-use-a.md: warning: the ADR became superseded since main; review \
             whether the decisions of the ADRs that still list it in `requires` still hold: \
             ADR-0002\n"
        ),
        "{}",
        stdout(&out)
    );
}

/// 確定した ADR の決定と理由を直した topic のブランチで、`item` を「## 変更」に足す（空なら足さない）。
fn decision_edited(name: &str, item: &str) -> TempRepo {
    let t = superseded_repo(name);
    t.git(&["switch", "-q", "-c", "topic"]);
    let b = "docs/adr/0002-use-b.md";
    let mut text = t
        .read(b)
        .replace("## 決定と理由\n", "## 決定と理由\n\n決定を直す\n");
    if !item.is_empty() {
        text.push_str(&format!("\n## 変更\n\n{item}\n"));
    }
    t.write(b, &text);
    t
}

const MISSING_CHANGE_ITEM: &str = "docs/adr/0002-use-b.md: warning: the decision outcome changed \
     but no item was added to the `## 変更` section; add `- YYYY-MM-DD: <changed part>。前は\
     「<quote of the previous text>」。<reason>`, or ignore this if the edit only fixes typos, \
     grammar, markup or links\n";

#[test]
fn check_warns_when_the_decision_changes_without_a_change_item() {
    let t = decision_edited("no-item", "");
    let out = t.run(&["check", "docs/adr/0002-use-b.md"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(
        stdout(&out).contains(MISSING_CHANGE_ITEM),
        "{}",
        stdout(&out)
    );
}

#[test]
fn check_accepts_a_decision_change_with_a_change_item() {
    let t = decision_edited("with-item", "- 2026-11-01: 決定を直した。前は「決」。理由");
    let out = t.run(&["check", "docs/adr/0002-use-b.md"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(
        !stdout(&out).contains(MISSING_CHANGE_ITEM),
        "{}",
        stdout(&out)
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

/// topic のブランチで ADR-0001 を足し、その「確認」に `src/impl.sh` を挙げ、実装を足した後、`later` を実行する
fn confirmed_topic(name: &str, later: impl Fn(&TempRepo)) -> TempRepo {
    let t = TempRepo::new(name);
    t.write("src/impl.sh", "echo 1\n");
    t.commit_all("feat: impl");
    t.git(&["switch", "-q", "-c", "topic"]);
    t.run(&["new", "use-a", "a を使う"]);
    let a = "docs/adr/0001-use-a.md";
    t.write(
        a,
        &t.read(a)
            .replace("### 確認\n", "### 確認\n\n- `src/impl.sh` が 1 を出す\n"),
    );
    t.commit_all("docs(adr): a");
    later(&t);
    t
}

#[test]
fn check_warns_when_a_file_named_in_the_confirmation_changed_after_the_adr() {
    let t = confirmed_topic("stale-confirmation", |t| {
        t.write("src/impl.sh", "echo 2\n");
        t.commit_all("fix: impl");
    });
    let out = t.run(&["check"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert_eq!(
        stdout(&out),
        "docs/adr/0001-use-a.md: warning: `src/impl.sh`, named in the confirmation section, \
         changed in a later commit than the ADR; check that the decision, consequences and \
         confirmation still match it\n"
    );
}

#[test]
fn check_keeps_the_stale_confirmation_after_the_adr_is_renumbered() {
    let t = confirmed_topic("stale-renumbered", |t| {
        t.write("src/impl.sh", "echo 2\n");
        t.commit_all("fix: impl");
    });
    let out = t.run(&["renumber", "1", "5"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let out = t.run(&["check"]);
    assert!(
        stdout(&out).contains(
            "docs/adr/0005-use-a.md: warning: `src/impl.sh`, named in the confirmation section"
        ),
        "{}",
        stdout(&out)
    );
}

#[test]
fn check_does_not_count_the_references_renumber_rewrites_as_a_change() {
    let t = confirmed_topic("stale-renumber-refs", |t| {
        // ブランチで足した行の ADR-0001 の参照は、renumber が付け替える
        t.write("src/impl.sh", "echo 1\n# ADR-0001\n");
        t.commit_all("fix: impl");
        let a = "docs/adr/0001-use-a.md";
        t.write(a, &(t.read(a) + "\n追記\n"));
        t.commit_all("docs(adr): follow impl");
    });
    let out = t.run(&["renumber", "1", "5"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(t.read("src/impl.sh").contains("ADR-0005"));
    let out = t.run(&["check"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert_eq!(stdout(&out), "");
}

#[test]
fn check_counts_a_hand_written_commit_that_starts_like_renumber() {
    let t = confirmed_topic("stale-renumber-like", |t| {
        t.write("src/impl.sh", "echo 2\n");
        t.commit_all("fix: impl");
        let a = "docs/adr/0001-use-a.md";
        t.write(a, &t.read(a).replace("1 を出す", "2 を出す"));
        t.commit_all("docs: renumber the steps in the confirmation");
    });
    let out = t.run(&["check"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert_eq!(stdout(&out), "");
}

#[test]
fn check_ignores_changes_merged_from_the_default_branch() {
    let t = confirmed_topic("stale-merged", |t| {
        t.git(&["switch", "-q", "main"]);
        t.write("src/impl.sh", "echo 3\n");
        t.commit_all("fix: impl on main");
        t.git(&["switch", "-q", "topic"]);
        t.git(&["merge", "-q", "--no-edit", "main"]);
    });
    let out = t.run(&["check"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert_eq!(stdout(&out), "");
}

#[test]
fn check_changed_shows_the_stale_confirmation_only_for_the_given_adr() {
    let t = confirmed_topic("stale-changed", |t| {
        t.write("src/impl.sh", "echo 2\n");
        t.commit_all("fix: impl");
    });
    let out = t.run(&["check", "--changed"]);
    assert_eq!(stdout(&out), "");
    let out = t.run(&["check", "--changed", "docs/adr/0001-use-a.md"]);
    assert!(
        stdout(&out).contains("`src/impl.sh`, named in the confirmation"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn check_is_quiet_when_the_adr_follows_the_file_only_in_the_working_tree() {
    let t = confirmed_topic("stale-uncommitted", |t| {
        t.write("src/impl.sh", "echo 2\n");
        t.commit_all("fix: impl");
        t.write(
            "docs/adr/0001-use-a.md",
            &(t.read("docs/adr/0001-use-a.md") + "\n追記\n"),
        );
    });
    let out = t.run(&["check"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert_eq!(stdout(&out), "");
}

#[test]
fn check_ignores_directories_named_in_the_confirmation() {
    let t = confirmed_topic("stale-directory", |t| {
        let a = "docs/adr/0001-use-a.md";
        t.write(a, &t.read(a).replace("`src/impl.sh` が", "`src/` の下が"));
        t.commit_all("docs(adr): directory");
        t.write("src/impl.sh", "echo 2\n");
        t.commit_all("fix: impl");
    });
    let out = t.run(&["check"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert_eq!(stdout(&out), "");
}

#[test]
fn check_is_quiet_when_the_adr_follows_the_file_named_in_the_confirmation() {
    let t = confirmed_topic("fresh-confirmation", |t| {
        t.write("src/impl.sh", "echo 2\n");
        t.commit_all("fix: impl");
        t.write(
            "docs/adr/0001-use-a.md",
            &(t.read("docs/adr/0001-use-a.md") + "\n追記\n"),
        );
        t.commit_all("docs(adr): follow impl");
    });
    let out = t.run(&["check"]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert_eq!(stdout(&out), "");
}

#[test]
fn check_ignores_confirmation_paths_that_changed_before_the_adr_or_do_not_exist() {
    let t = confirmed_topic("no-confirmation-change", |t| {
        let a = "docs/adr/0001-use-a.md";
        t.write(
            a,
            &t.read(a)
                .replace("`src/impl.sh` が", "`src/impl.sh` と `src/none.sh` が"),
        );
        t.commit_all("docs(adr): more");
    });
    let out = t.run(&["check"]);
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
fn check_rejects_amends_added_to_an_adr_on_main() {
    let t = TempRepo::new("added-link");
    t.run(&["new", "use-a", "a を使う"]);
    t.run(&["new", "use-b", "b を使う"]);
    let b = "docs/adr/0002-use-b.md";
    t.commit_all("docs(adr): a and b");
    t.git(&["switch", "-q", "-c", "topic"]);
    t.write(
        b,
        &t.read(b)
            .replace("superseded-by:\n", "superseded-by:\namends: [ADR-0001]\n"),
    );
    let out = t.run(&["check", b]);
    assert!(!out.status.success(), "{}", stdout(&out));
    assert_eq!(stdout(&out), new_link_error(b, 1));
}

#[test]
fn check_rejects_amends_added_since_main_without_files() {
    let t = TempRepo::new("added-link-all");
    t.run(&["new", "use-a", "a を使う"]);
    t.run(&["new", "use-b", "b を使う"]);
    let b = "docs/adr/0002-use-b.md";
    t.commit_all("docs(adr): a and b");
    t.git(&["switch", "-q", "-c", "topic"]);
    t.write(
        b,
        &t.read(b)
            .replace("superseded-by:\n", "superseded-by:\namends: [ADR-0001]\n"),
    );
    t.commit_all("docs(adr): link");
    let out = t.run(&["check"]);
    assert!(!out.status.success(), "{}", stdout(&out));
    assert_eq!(stdout(&out), new_link_error(b, 1));
    // FILE が別の ADR でも、error はすべて出す
    let out = t.run(&["check", "--changed", "docs/adr/0001-use-a.md"]);
    assert!(!out.status.success(), "{}", stdout(&out));
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

#[cfg(unix)]
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

#[cfg(unix)]
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

/// リポジトリの設定（core.autocrlf）をそのまま効かせて git を実行し、標準出力を返す
fn crlf_git(t: &TempRepo, args: &[&str]) -> String {
    let mut command = Command::new("git");
    command.current_dir(&t.dir).args(args);
    isolate(&mut command);
    let out = command.output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", stderr(&out));
    stdout(&out)
}

/// 作業ツリーを、`core.autocrlf=true` で checkout し直して CRLF にする（Git for Windows の既定。Linux の git でも効く）。
/// テストの git は GIT_CONFIG で変換を止めるので、設定はリポジトリのファイルに書き、CLI の git だけが読む
fn to_crlf_worktree(t: &TempRepo) {
    crlf_git(t, &["config", "core.autocrlf", "true"]);
    let tracked = t.git(&["ls-files"]);
    for path in tracked.lines() {
        fs::remove_file(t.dir.join(path)).unwrap();
    }
    crlf_git(t, &["checkout", "--", "."]);
}

#[test]
fn check_reads_crlf_front_matter_and_headings_like_lf() {
    let t = TempRepo::new("crlf-check");
    t.run(&["new", "use-foo", "foo を使う"]);
    t.commit_all("docs(adr): foo");
    to_crlf_worktree(&t);
    assert!(t.read("docs/adr/0001-use-foo.md").contains("---\r\n"));
    let out = t.run(&["check"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert_eq!(stdout(&out), "");
}

#[test]
fn new_writes_lf_in_a_crlf_worktree_and_check_accepts_it() {
    let t = TempRepo::new("crlf-new");
    t.run(&["new", "use-foo", "foo を使う"]);
    t.commit_all("docs(adr): foo");
    to_crlf_worktree(&t);
    let out = t.run(&["new", "use-bar", "bar を使う"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    let bar = t.read("docs/adr/0002-use-bar.md");
    assert!(bar.starts_with("---\n"), "{bar:?}");
    assert!(!bar.contains('\r'), "{bar:?}");
    t.git(&["add", "docs/adr/0002-use-bar.md"]);
    let out = t.run(&["check"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert_eq!(stdout(&out), "");
}

#[test]
fn renumber_keeps_crlf_in_a_crlf_worktree() {
    let t = duplicated_after_rebase("crlf-renumber");
    to_crlf_worktree(&t);
    assert_eq!(crlf_git(&t, &["status", "--porcelain"]), "");
    let out = t.run(&["renumber", "1"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    let b = t.read("docs/adr/0002-use-b.md");
    assert!(b.contains("# ADR-0002: b を使う\r\n"), "{b:?}");
    assert!(b.contains("[本文](0002-use-b.md) と ADR-0002\r\n"), "{b:?}");
    assert!(!b.contains("0001"), "{b:?}");
    assert!(
        t.read("notes.md")
            .starts_with("ADR-0002 の続き\r\n前置き\r\n"),
        "{:?}",
        t.read("notes.md")
    );
    assert_eq!(crlf_git(&t, &["status", "--porcelain"]), "");
    let out = t.run(&["check"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
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

/// ブランチ topic に記録が入った予約 adr-0001 と、記録を作らずに終わった予約 adr-0002
fn stale_reservations(name: &str) -> TempRepo {
    let t = TempRepo::with_remote(name);
    t.git(&["switch", "-q", "-c", "topic"]);
    t.run(&["new", "use-a", "a を使う"]);
    t.commit_all("docs(adr): a");
    t.git(&["switch", "-q", "main"]);
    t.run(&["new", "use-b", "b を使う"]);
    fs::remove_file(t.dir.join("docs/adr/0002-use-b.md")).unwrap();
    t
}

#[test]
fn prune_stale_lists_old_reservations_that_no_branch_recorded() {
    let t = stale_reservations("stale-list");
    // 既定の期間（30 日）より新しい予約は、まだ使うかもしれない
    let out = t.run(&["prune", "--stale"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "");
    let out = t.run(&["prune", "--stale", "--days", "0"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "adr-0002\tuse-b\t0 days\n");
    // 一覧だけで、消さない
    assert_eq!(t.tag_message("adr-0002"), "use-b");
    assert!(t.remote_tag("adr-0002").contains("adr-0002"));
}

#[test]
fn prune_stale_delete_releases_them_locally_and_on_origin() {
    let t = stale_reservations("stale-delete");
    let out = t.run(&["prune", "--stale", "--days", "0", "--delete"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "adr-0002\n");
    assert_eq!(t.git(&["tag", "--list", "adr-0002"]), "");
    assert_eq!(t.remote_tag("adr-0002"), "");
    // ブランチに記録のある予約は残す
    assert_eq!(t.tag_message("adr-0001"), "use-a");
    assert!(t.remote_tag("adr-0001").contains("adr-0001"));
}

#[test]
fn prune_stale_sees_records_on_branches_only_origin_has() {
    let t = stale_reservations("stale-origin-branch");
    let other = t.clone_remote("other");
    git_in(&other, &["switch", "-q", "-c", "elsewhere"]);
    fs::create_dir_all(other.join("docs/adr")).unwrap();
    fs::write(other.join("docs/adr/0002-use-b.md"), "# ADR-0002: b\n").unwrap();
    git_in(&other, &["add", "-A"]);
    git_in(&other, &["commit", "-q", "-m", "docs(adr): b"]);
    git_in(&other, &["push", "-q", "origin", "elsewhere"]);
    let out = t.run(&["prune", "--stale", "--days", "0"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "");
}

#[cfg(unix)]
#[test]
fn prune_stale_delete_keeps_a_reservation_another_clone_remade_after_the_scan() {
    let t = stale_reservations("stale-race");
    let other = t.clone_remote("other");
    // origin のタグの一覧の直後に、ほかの clone が古い予約を消して同じ番号を別の slug で予約し直す git
    let mark = t.root.join("listed");
    let bin = t.root.join("bin");
    fake_git(
        &bin,
        &format!(
            "  *\" ls-remote --tags \"*)\n\
             \x20   if [ ! -e {mark} ]; then : > {mark}; \"$GIT\" \"$@\"; \
             \"$GIT\" -C {other} push -q origin --delete refs/tags/adr-0002; \
             \"$GIT\" -C {other} tag -f -a adr-0002 -m use-c main; \
             \"$GIT\" -C {other} push -q origin refs/tags/adr-0002; exit 0; fi ;;",
            mark = sh_quote(mark.display()),
            other = sh_quote(other.display()),
        ),
    );
    let out = t.run_with_bin(&bin, &["prune", "--stale", "--days", "0", "--delete"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(mark.exists());
    assert_eq!(stdout(&out), "");
    let remote = git_in(&other, &["rev-parse", "refs/tags/adr-0002"]);
    assert!(t.remote_tag("adr-0002").starts_with(remote.trim()));
    // 手元のタグも、スキャンした予約でなくなったものは消さない
    assert_eq!(t.tag_message("adr-0002"), "use-b");
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
fn renumber_reads_paths_with_spaces() {
    let t = duplicated_after_rebase("paths");
    t.write("plans/a plan.md", "ADR-0001 の計画\n");
    t.commit_all("docs: plan");
    let out = t.run(&["renumber", "1"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert_eq!(t.read("plans/a plan.md"), "ADR-0002 の計画\n");
    assert_eq!(t.git(&["status", "--porcelain"]), "");
}

// Windows の symlink は管理者の権限か開発者モードが要るので、Unix だけで作る
#[cfg(unix)]
#[test]
fn renumber_skips_symlinks() {
    let t = duplicated_after_rebase("symlinks");
    // リポジトリの外のファイルへのリンク
    let outside = t.root.join("outside.md");
    fs::write(&outside, "ADR-0001 は a\n").unwrap();
    std::os::unix::fs::symlink(&outside, t.dir.join("link.md")).unwrap();
    t.commit_all("docs: link");
    let out = t.run(&["renumber", "1"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
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
    // Git for Windows は実行ビットを見ずに hook を同梱の sh で実行する
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    }
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

/// 基準の無い renumber を、`GIT_TRACE` を付けた git で実行する（git grep の標準エラーに trace が出る）
#[cfg(unix)]
#[test]
fn renumber_without_a_common_ancestor_ignores_git_diagnostics_on_stderr() {
    let t = TempRepo::new("no-ancestor-trace");
    t.git(&["checkout", "-q", "--orphan", "lone"]);
    t.run(&["new", "use-a", "a を使う"]);
    t.write("notes.md", "ADR-0001 を見る\n");
    t.commit_all("docs(adr): a");
    let mut command = Command::new(env!("CARGO_BIN_EXE_design-records"));
    command.current_dir(&t.dir).args(["renumber", "1", "5"]);
    isolate(&mut command);
    command.env("GIT_TRACE", "1");
    let out = command.output().unwrap();
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert_eq!(t.read("notes.md"), "ADR-0005 を見る\n");
}

/// assume-unchanged のエントリーは、git grep が index のブロブを読むので、作業ツリーだけにある旧番号を見落とす。
/// skip-worktree は git add が受け付けない（sparse-checkout の外）ので対象にしない
#[test]
fn renumber_without_a_common_ancestor_rewrites_assume_unchanged_files() {
    let t = TempRepo::new("no-ancestor-assume-unchanged");
    t.git(&["checkout", "-q", "--orphan", "lone"]);
    t.run(&["new", "use-a", "a を使う"]);
    t.write("notes.md", "nothing\n");
    t.commit_all("docs(adr): a");
    t.git(&["update-index", "--assume-unchanged", "notes.md"]);
    t.write("notes.md", "ADR-0001 を見る\n");
    let out = t.run(&["renumber", "1", "5"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert_eq!(t.read("notes.md"), "ADR-0005 を見る\n");
}

// 読めないファイルを mode 000 で作る。Windows の権限は mode で表せない
#[cfg(unix)]
#[test]
fn renumber_without_a_common_ancestor_stops_when_a_mentioning_file_cannot_be_read() {
    let t = TempRepo::new("no-ancestor-unreadable");
    t.git(&["checkout", "-q", "--orphan", "lone"]);
    t.run(&["new", "use-a", "a を使う"]);
    t.write("notes.md", "ADR-0001 を見る\n");
    // index より十分前の更新時刻で入れ、git が中身を読み直さずに（racy-git の確認なしに）変更なしと判断できるようにする
    let notes = t.dir.join("notes.md");
    fs::File::options()
        .write(true)
        .open(&notes)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
        .unwrap();
    t.commit_all("docs(adr): a");
    t.git(&["config", "core.fileMode", "false"]);
    t.git(&["config", "core.trustctime", "false"]);
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&notes, fs::Permissions::from_mode(0o000)).unwrap();
    // root は mode 000 のファイルも読めるので、この状況を作れない（Podman のコンテナなど）
    if fs::File::open(&notes).is_ok() {
        fs::set_permissions(&notes, fs::Permissions::from_mode(0o644)).unwrap();
        eprintln!("skipped: mode 000 のファイルを読めてしまう（root で実行している）");
        return;
    }
    let out = t.run(&["renumber", "1", "5"]);
    fs::set_permissions(&notes, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(!out.status.success(), "{}", stdout(&out));
    assert!(t.dir.join("docs/adr/0001-use-a.md").exists());
}

#[cfg(unix)]
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

const OKF_ROOT_INDEX: &str = "---\nokf_version: \"0.2\"\n---\n\n# docs\n";

#[test]
fn check_reports_okf_findings_only_in_a_bundle() {
    let t = TempRepo::new("okf-check");
    t.write("docs/note.md", "no front matter\n");
    t.commit_all("docs: note");
    assert!(t.run(&["check"]).status.success());

    t.write("docs/index.md", OKF_ROOT_INDEX);
    t.commit_all("docs: bundle root");
    let out = t.run(&["check"]);
    assert!(!out.status.success());
    let shown = stdout(&out);
    assert!(shown.contains("docs/note.md: error"), "{shown}");
    assert!(shown.contains("`note.md` is not listed"), "{shown}");
}

#[test]
fn index_completes_the_bundle_and_check_then_passes() {
    let t = TempRepo::new("okf-index");
    t.write("docs/index.md", OKF_ROOT_INDEX);
    t.write(
        "docs/windows.md",
        "---\ntype: Guide\ntitle: Windows\ndescription: 入れ方\n---\n",
    );
    // new も索引を足すので、index の確かめには ADR を手で書く
    t.write(
        "docs/adr/0001-use-foo.md",
        &design_records::template(1, "foo を使う", &today()),
    );
    let out = t.run(&["index"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "docs/adr/index.md\ndocs/index.md\n");
    assert_eq!(
        t.read("docs/adr/index.md"),
        "# adr\n\n* [ADR-0001: foo を使う](0001-use-foo.md)\n"
    );
    t.commit_all("docs: bundle");
    let out = t.run(&["check"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    assert!(t.run(&["index"]).stdout.is_empty());
}

#[test]
fn check_does_not_read_docs_outside_a_bundle() {
    let t = TempRepo::new("okf-nonbundle");
    t.write("docs/index.md", "# docs\n");
    fs::write(t.dir.join("docs/binary.md"), [0xff, 0xfe, 0x00]).unwrap();
    t.commit_all("docs: not utf-8");
    let out = t.run(&["check"]);
    assert!(out.status.success(), "{}{}", stdout(&out), stderr(&out));
    let out = t.run(&["index"]);
    assert!(out.status.success());
    assert!(
        stderr(&out).contains("not an OKF bundle"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn new_in_a_bundle_adds_the_index_lines_and_names_them_to_commit() {
    let t = TempRepo::new("okf-new");
    t.write("docs/index.md", OKF_ROOT_INDEX);
    t.commit_all("docs: bundle root");
    let out = t.run(&["new", "use-foo", "foo を使う"]);
    assert!(out.status.success(), "{}", stderr(&out));
    // 標準出力は記録のパスだけ（`$(design-records new …)` で受ける呼び出しを壊さない）
    assert_eq!(stdout(&out), "docs/adr/0001-use-foo.md\n");
    assert_eq!(
        t.read("docs/adr/index.md"),
        "# adr\n\n* [ADR-0001: foo を使う](0001-use-foo.md)\n"
    );
    assert!(
        stderr(&out).contains(
            "NEXT: commit the index lines added to docs/adr/index.md docs/index.md together with \
             docs/adr/0001-use-foo.md"
        ),
        "{}",
        stderr(&out)
    );
    assert!(t.run(&["index"]).stdout.is_empty());
}

#[test]
fn new_in_a_bundle_leaves_untracked_drafts_out_of_the_index() {
    let t = TempRepo::new("okf-new-draft");
    t.write("docs/index.md", OKF_ROOT_INDEX);
    t.commit_all("docs: bundle root");
    t.write(
        "docs/plans/wip.md",
        "---\ntype: Plan\ntitle: wip\ndescription: 下書き\n---\n",
    );
    // 索引まで書いた下書きのディレクトリも、記録の索引ではないので載せない
    t.write("docs/drafts/index.md", "# drafts\n");
    t.write(
        "docs/drafts/idea.md",
        "---\ntype: Note\ntitle: idea\ndescription: 案\n---\n",
    );
    let out = t.run(&["new", "use-foo", "foo を使う"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(!t.dir.join("docs/plans/index.md").exists());
    assert!(
        !t.read("docs/index.md").contains("plans/"),
        "{}",
        t.read("docs/index.md")
    );
    assert!(
        !t.read("docs/index.md").contains("drafts/"),
        "{}",
        t.read("docs/index.md")
    );
    assert_eq!(t.read("docs/drafts/index.md"), "# drafts\n");
    // 続けて作った記録は、先の new が書いた（まだ追跡していない）索引に足す
    t.run(&["new", "use-bar", "bar を使う"]);
    assert_eq!(
        t.read("docs/adr/index.md"),
        "# adr\n\n* [ADR-0001: foo を使う](0001-use-foo.md)\n* [ADR-0002: bar を使う](0002-use-bar.md)\n"
    );
}

#[test]
fn new_keeps_the_record_when_the_index_cannot_be_written() {
    let t = TempRepo::new("okf-new-broken");
    t.write("docs/index.md", OKF_ROOT_INDEX);
    fs::write(t.dir.join("docs/broken.md"), [0xff, 0xfe, 0x00]).unwrap();
    t.commit_all("docs: bundle with a non-utf-8 file");
    let out = t.run(&["new", "use-foo", "foo を使う"]);
    // 番号は予約して記録も書いたので、成功で終えてパスを出し、索引の失敗と修正方法を示す
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "docs/adr/0001-use-foo.md\n");
    assert!(
        stderr(&out).contains("NEXT: fix it, run `design-records index`"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn new_outside_a_bundle_writes_no_index() {
    let t = TempRepo::new("okf-new-plain");
    let out = t.run(&["new", "use-foo", "foo を使う"]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stderr(&out), "");
    assert!(!t.dir.join("docs/adr/index.md").exists());
}
