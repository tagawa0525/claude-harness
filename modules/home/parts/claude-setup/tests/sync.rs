//! `claude-setup sync` が、ハーネスの .claude（CLAUDE.md、commands、skills、scripts）を ~/.claude に同期すること
//! （ADR-0084）。
//!
//! - CLAUDE.md、skills、scripts はソースが正として、中身の違うファイルを上書きする（サイズと mtime が同じでも）
//! - commands は既にあるファイルを残す。どこも、ソースに無いファイルを消さない
//! - 引数なしは既定の clone（ホームの下の nix/claude-harness）を、既定のブランチで、同期する場所に未コミットの変更が
//!   無ければ `git pull --ff-only` してから同期する。どれかに当たれば、~/.claude を変えずに 1 で止まる
//! - 引数を渡せば pull せず、その場所の今の中身を同期する

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

/// 一時のホームと、そこで動かす git と claude-setup。テストが落ちても消す（親を指すリンクを target の下に
/// 残すと、target を辿るツールが止まらなくなる）
struct Home(PathBuf);

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Home {
    fn new() -> Home {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "sync-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("home")).unwrap();
        std::fs::write(
            dir.join("home/.gitconfig"),
            "[user]\n\tname = test\n\temail = test@example.com\n[init]\n\tdefaultBranch = main\n",
        )
        .unwrap();
        Home(dir)
    }

    fn home(&self) -> PathBuf {
        self.0.join("home")
    }

    fn dest(&self, rel: &str) -> PathBuf {
        self.home().join(".claude").join(rel)
    }

    fn clone_dir(&self) -> PathBuf {
        self.home().join("nix/claude-harness")
    }

    fn env(&self, command: &mut Command) {
        // 外の git（hook の中で走るテストや git -c）が渡す GIT_* を外す（ほかの crate のテストと同じ）
        for (name, _) in std::env::vars_os() {
            if name.to_string_lossy().starts_with("GIT_") {
                command.env_remove(name);
            }
        }
        command
            .env("HOME", self.home())
            .env("USERPROFILE", self.home())
            .env("GIT_CONFIG_GLOBAL", self.home().join(".gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1");
    }

    fn git(&self, dir: &Path, args: &[&str]) {
        let mut command = Command::new("git");
        self.env(&mut command);
        let out = command.current_dir(dir).args(args).output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn sync(&self, source: Option<&Path>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_claude-setup"));
        self.env(&mut command);
        command.arg("sync");
        if let Some(source) = source {
            command.arg(source);
        }
        command.current_dir(&self.0).output().unwrap()
    }

    /// 引数なしの同期が成功すること
    fn sync_default(&self) {
        let out = self.sync(None);
        assert!(out.status.success(), "{}", stderr(&out));
    }

    /// 引数なしの同期が、1 で、理由に want を含めて止まり、~/.claude の CLAUDE.md を変えないこと
    fn stops(&self, want: &str) {
        let before = read(&self.dest("CLAUDE.md"));
        let out = self.sync(None);
        assert_eq!(out.status.code(), Some(1), "{}", stderr(&out));
        assert!(stderr(&out).contains(want), "{want}: {}", stderr(&out));
        assert_eq!(read(&self.dest("CLAUDE.md")), before);
    }
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|_| "(ファイルが無い)".to_string())
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// clone の CLAUDE.md を書き換えてコミットし、origin に push する
fn advance(home: &Home, clone: &Path, text: &str) {
    write(&clone.join(".claude/CLAUDE.md"), text);
    home.git(clone, &["add", ".claude/CLAUDE.md"]);
    home.git(clone, &["commit", "-qm", &format!("docs: {text}")]);
    home.git(clone, &["push", "-q", "origin", "HEAD"]);
}

/// origin（bare）と、そこから clone した既定の clone と、もう 1 つの clone（origin を進める）を作る
fn clones(home: &Home) -> PathBuf {
    let origin = home.0.join("origin.git");
    home.git(&home.0, &["init", "-q", "--bare", origin.to_str().unwrap()]);
    let clone = home.clone_dir();
    std::fs::create_dir_all(clone.parent().unwrap()).unwrap();
    home.git(
        &home.0,
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    advance(home, &clone, "v1");
    let other = home.0.join("other");
    home.git(
        &home.0,
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    other
}

#[test]
fn overwrites_changed_files_and_keeps_existing_commands() {
    let home = Home::new();
    let src = home.0.join("src");
    for (rel, text) in [
        ("CLAUDE.md", "ADR-0175"),
        ("skills/s/SKILL.md", "ADR-0175"),
        ("scripts/a.sh", "ADR-0175"),
        ("commands/c.md", "source"),
    ] {
        write(&src.join(".claude").join(rel), text);
    }
    assert!(home.sync(Some(&src)).status.success());
    // 利用者が手で直した commands は残す
    write(&home.dest("commands/c.md"), "user");
    // 同じサイズで中身だけ変える
    for (rel, text) in [
        ("CLAUDE.md", "ADR-NNNN"),
        ("skills/s/SKILL.md", "ADR-NNNN"),
        ("scripts/a.sh", "ADR-NNNN"),
        ("commands/c.md", "change"),
    ] {
        write(&src.join(".claude").join(rel), text);
    }
    let out = home.sync(Some(&src));
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(read(&home.dest("CLAUDE.md")), "ADR-NNNN");
    assert_eq!(read(&home.dest("skills/s/SKILL.md")), "ADR-NNNN");
    assert_eq!(read(&home.dest("scripts/a.sh")), "ADR-NNNN");
    assert_eq!(read(&home.dest("commands/c.md")), "user");
}

#[test]
fn keeps_files_the_source_does_not_have() {
    let home = Home::new();
    let src = home.0.join("src");
    write(&src.join(".claude/skills/s/SKILL.md"), "s");
    write(&home.dest("skills/mine/SKILL.md"), "mine");
    assert!(home.sync(Some(&src)).status.success());
    assert_eq!(read(&home.dest("skills/mine/SKILL.md")), "mine");
}

#[cfg(unix)]
#[test]
fn keeps_the_executable_bit_and_makes_read_only_copies_writable() {
    use std::os::unix::fs::PermissionsExt;
    let home = Home::new();
    let src = home.0.join("src");
    let script = src.join(".claude/scripts/run.sh");
    write(&script, "#!/bin/sh\n");
    // nix store のソースは読むだけ
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o555)).unwrap();
    assert!(home.sync(Some(&src)).status.success());
    let mode = std::fs::metadata(home.dest("scripts/run.sh"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o700, 0o700, "{mode:o}");
    // 読むだけのコピーの上にも、次の版を書ける
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    write(&script, "#!/bin/sh\necho v2\n");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o555)).unwrap();
    assert!(home.sync(Some(&src)).status.success());
    assert_eq!(read(&home.dest("scripts/run.sh")), "#!/bin/sh\necho v2\n");
}

#[test]
fn without_arguments_syncs_the_harness_clone_not_nixfiles() {
    let home = Home::new();
    write(&home.clone_dir().join(".claude/CLAUDE.md"), "harness");
    write(
        &home.home().join("nix/nixfiles/.claude/CLAUDE.md"),
        "nixfiles project",
    );
    home.sync_default();
    assert_eq!(read(&home.dest("CLAUDE.md")), "harness");
}

#[test]
fn without_arguments_pulls_the_clone_first() {
    let home = Home::new();
    let other = clones(&home);
    advance(&home, &other, "v2");
    let out = home.sync(None);
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(String::from_utf8_lossy(&out.stdout).contains("📥 "));
    assert_eq!(read(&home.dest("CLAUDE.md")), "v2");
}

#[test]
fn without_arguments_stops_off_the_default_branch_or_with_drafts() {
    let home = Home::new();
    let _other = clones(&home);
    home.sync_default();
    let clone = home.clone_dir();
    let clone_path = clone.display().to_string();
    home.git(&clone, &["switch", "-qc", "feat/x"]);
    home.stops(&format!("git -C {clone_path} switch main"));
    home.git(&clone, &["switch", "-q", "--detach", "main"]);
    home.stops(&format!("git -C {clone_path} switch main"));
    home.git(&clone, &["switch", "-q", "main"]);
    write(&clone.join(".claude/CLAUDE.md"), "draft");
    home.stops(&format!("claude-sync {clone_path}"));
    home.git(&clone, &["restore", ".claude/CLAUDE.md"]);
    // 同期しない場所の書きかけでは止めない
    write(&clone.join(".claude/notes.txt"), "memo");
    home.sync_default();
    std::fs::remove_file(clone.join(".claude/notes.txt")).unwrap();
    // 同期する場所の、追跡していない新しいファイルでは止める
    write(&clone.join(".claude/skills/new/SKILL.md"), "wip");
    home.stops(&format!("claude-sync {clone_path}"));
}

#[test]
fn with_a_path_syncs_the_current_contents_without_pulling() {
    let home = Home::new();
    let other = clones(&home);
    let clone = home.clone_dir();
    write(&clone.join(".claude/CLAUDE.md"), "draft");
    advance(&home, &other, "v3");
    let out = home.sync(Some(&clone));
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(read(&home.dest("CLAUDE.md")), "draft");
}

#[test]
fn without_arguments_stops_when_the_clone_cannot_pull() {
    let home = Home::new();
    let other = clones(&home);
    home.sync_default();
    advance(&home, &other, "v2");
    let clone = home.clone_dir();
    write(&clone.join(".claude/CLAUDE.md"), "local");
    home.git(&clone, &["commit", "-qam", "docs: local"]);
    home.stops(&format!("claude-sync {}", clone.display()));
}

#[test]
fn the_default_branch_comes_from_origin_head() {
    let home = Home::new();
    let origin = home.0.join("origin2.git");
    home.git(
        &home.0,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "trunk",
            origin.to_str().unwrap(),
        ],
    );
    let seed = home.0.join("seed");
    home.git(
        &home.0,
        &["init", "-q", "-b", "trunk", seed.to_str().unwrap()],
    );
    home.git(
        &seed,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    advance(&home, &seed, "trunk1");
    let clone = home.clone_dir();
    std::fs::create_dir_all(clone.parent().unwrap()).unwrap();
    home.git(
        &home.0,
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    home.sync_default();
    assert_eq!(read(&home.dest("CLAUDE.md")), "trunk1");
    home.git(&clone, &["switch", "-qc", "main"]);
    home.stops(&format!("git -C {} switch trunk", clone.display()));
}

#[test]
fn an_inherited_git_dir_does_not_redirect_the_default_clone() {
    let home = Home::new();
    let other = clones(&home);
    let elsewhere = home.0.join("elsewhere");
    home.git(&home.0, &["init", "-q", elsewhere.to_str().unwrap()]);
    home.git(
        &elsewhere,
        &["commit", "-q", "--allow-empty", "-m", "chore: init"],
    );
    advance(&home, &other, "v4");
    let mut command = Command::new(env!("CARGO_BIN_EXE_claude-setup"));
    home.env(&mut command);
    let out = command
        .arg("sync")
        .env("GIT_DIR", elsewhere.join(".git"))
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(read(&home.dest("CLAUDE.md")), "v4");
}

#[test]
fn a_default_place_that_is_not_a_clone_is_synced_as_is() {
    let home = Home::new();
    // 親のリポジトリの中の、git でないディレクトリ
    home.git(&home.home(), &["init", "-q", "nix"]);
    write(&home.clone_dir().join(".claude/CLAUDE.md"), "plain copy");
    home.sync_default();
    assert_eq!(read(&home.dest("CLAUDE.md")), "plain copy");
}

#[test]
fn a_missing_source_is_an_error() {
    let home = Home::new();
    let out = home.sync(Some(&home.0.join("missing")));
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("ソースが見つかりません"));
}

#[test]
fn an_empty_argument_means_the_default_clone() {
    // 空の変数を渡したとき（claude-sync "$REPO"）に、今のディレクトリの .claude を同期しない
    let home = Home::new();
    write(&home.clone_dir().join(".claude/CLAUDE.md"), "harness");
    write(&home.0.join(".claude/CLAUDE.md"), "cwd project");
    let out = home.sync(Some(Path::new("")));
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(read(&home.dest("CLAUDE.md")), "harness");
}

#[cfg(unix)]
#[test]
fn symbolic_links_are_copied_as_links() {
    let home = Home::new();
    let src = home.0.join("src");
    write(&src.join(".claude/skills/s/SKILL.md"), "s");
    std::os::unix::fs::symlink("s", src.join(".claude/skills/alias")).unwrap();
    // 親を指すリンクで止まらない
    std::os::unix::fs::symlink("..", src.join(".claude/skills/s/up")).unwrap();
    let out = home.sync(Some(&src));
    assert!(out.status.success(), "{}", stderr(&out));
    let link = home.dest("skills/alias");
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read_link(&link).unwrap(), Path::new("s"));
}

#[cfg(unix)]
#[test]
fn files_only_in_the_destination_become_writable() {
    use std::os::unix::fs::PermissionsExt;
    let home = Home::new();
    let src = home.0.join("src");
    write(&src.join(".claude/skills/s/SKILL.md"), "s");
    let old = home.dest("skills/old/SKILL.md");
    write(&old, "old");
    std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o444)).unwrap();
    assert!(home.sync(Some(&src)).status.success());
    let mode = std::fs::metadata(&old).unwrap().permissions().mode();
    assert_eq!(mode & 0o200, 0o200, "{mode:o}");
}

#[cfg(unix)]
#[test]
fn a_mode_change_alone_reaches_the_destination() {
    use std::os::unix::fs::PermissionsExt;
    let home = Home::new();
    let src = home.0.join("src");
    let script = src.join(".claude/scripts/run.sh");
    write(&script, "#!/bin/sh\n");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(home.sync(Some(&src)).status.success());
    // 中身を変えずに実行の bit だけを足す（rsync -a は mode も合わせる）
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(home.sync(Some(&src)).status.success());
    let mode = std::fs::metadata(home.dest("scripts/run.sh"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o111, 0o111, "{mode:o}");
}

#[cfg(unix)]
#[test]
fn a_symlink_in_the_destination_is_replaced_not_written_through() {
    let home = Home::new();
    let src = home.0.join("src");
    write(&src.join(".claude/skills/foo/SKILL.md"), "harness");
    write(&src.join(".claude/skills/ref.md"), "file");
    // 利用者の作業の場所を指すリンクと、ファイルと同じ中身のファイルを指すリンク
    let work = home.0.join("work");
    write(&work.join("SKILL.md"), "mine");
    write(&home.0.join("same.md"), "file");
    std::fs::create_dir_all(home.dest("skills")).unwrap();
    std::os::unix::fs::symlink(&work, home.dest("skills/foo")).unwrap();
    std::os::unix::fs::symlink(home.0.join("same.md"), home.dest("skills/ref.md")).unwrap();
    assert!(home.sync(Some(&src)).status.success());
    assert_eq!(read(&work.join("SKILL.md")), "mine");
    for rel in ["skills/foo", "skills/ref.md"] {
        let kind = std::fs::symlink_metadata(home.dest(rel))
            .unwrap()
            .file_type();
        assert!(!kind.is_symlink(), "{rel}");
    }
    assert_eq!(read(&home.dest("skills/foo/SKILL.md")), "harness");
}

#[test]
fn a_file_and_a_directory_can_swap_places() {
    // ソースで、ファイルがディレクトリになり、ディレクトリ（空）がファイルになる（rsync -a は入れ替えた）
    let home = Home::new();
    let src = home.0.join("src");
    write(&src.join(".claude/skills/foo"), "file");
    std::fs::create_dir_all(src.join(".claude/skills/bar")).unwrap();
    assert!(home.sync(Some(&src)).status.success());
    std::fs::remove_file(src.join(".claude/skills/foo")).unwrap();
    write(&src.join(".claude/skills/foo/SKILL.md"), "dir");
    std::fs::remove_dir(src.join(".claude/skills/bar")).unwrap();
    write(&src.join(".claude/skills/bar"), "file");
    let out = home.sync(Some(&src));
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(read(&home.dest("skills/foo/SKILL.md")), "dir");
    assert_eq!(read(&home.dest("skills/bar")), "file");
}

#[cfg(unix)]
#[test]
fn top_level_places_are_followed_as_rsync_with_a_trailing_slash() {
    // ソースの skills が別の場所を指すリンクなら中身を、~/.claude の skills が利用者の場所を指すリンクなら
    // その中に同期する（rsync -a "$SRC/skills/" "$DEST/skills/"）
    let home = Home::new();
    let src = home.0.join("src");
    write(&home.0.join("shared/s/SKILL.md"), "shared");
    std::fs::create_dir_all(src.join(".claude")).unwrap();
    std::os::unix::fs::symlink(home.0.join("shared"), src.join(".claude/skills")).unwrap();
    let mine = home.0.join("mine");
    std::fs::create_dir_all(&mine).unwrap();
    std::fs::create_dir_all(home.dest("")).unwrap();
    std::os::unix::fs::symlink(&mine, home.dest("skills")).unwrap();
    let out = home.sync(Some(&src));
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        std::fs::symlink_metadata(home.dest("skills"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(read(&mine.join("s/SKILL.md")), "shared");
}

#[cfg(unix)]
#[test]
fn a_directory_that_became_a_link_is_replaced() {
    let home = Home::new();
    let src = home.0.join("src");
    std::fs::create_dir_all(src.join(".claude/skills/s/ref")).unwrap();
    write(&src.join(".claude/skills/s/SKILL.md"), "s");
    assert!(home.sync(Some(&src)).status.success());
    std::fs::remove_dir(src.join(".claude/skills/s/ref")).unwrap();
    std::os::unix::fs::symlink("SKILL.md", src.join(".claude/skills/s/ref")).unwrap();
    let out = home.sync(Some(&src));
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(
        std::fs::symlink_metadata(home.dest("skills/s/ref"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[cfg(unix)]
#[test]
fn places_the_source_lacks_become_writable_too() {
    use std::os::unix::fs::PermissionsExt;
    let home = Home::new();
    let src = home.0.join("src");
    write(&src.join(".claude/CLAUDE.md"), "x");
    let old = home.dest("commands/old.md");
    write(&old, "old");
    std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o444)).unwrap();
    assert!(home.sync(Some(&src)).status.success());
    let mode = std::fs::metadata(&old).unwrap().permissions().mode();
    assert_eq!(mode & 0o200, 0o200, "{mode:o}");
}

#[test]
fn help_prints_the_usage() {
    let home = Home::new();
    let mut command = Command::new(env!("CARGO_BIN_EXE_claude-setup"));
    home.env(&mut command);
    let out = command.args(["sync", "--help"]).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("Usage: claude-setup sync"));
}

#[test]
fn a_git_that_cannot_start_is_shown() {
    let home = Home::new();
    write(&home.clone_dir().join(".claude/CLAUDE.md"), "x");
    std::fs::create_dir_all(home.clone_dir().join(".git")).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_claude-setup"));
    home.env(&mut command);
    let out = command.arg("sync").env("PATH", "").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        stderr(&out).contains("git を起動できません"),
        "{}",
        stderr(&out)
    );
}
