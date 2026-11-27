//! `claude-setup git-attributes --from <ファイル>` が、git が既定で読むグローバルの attributes の場所にそのファイルを置き、
//! 利用者の attributes（ファイルと core.attributesFile）を奪わないことを確かめる（ADR-0105。Linux は git-attributes.nix が置く）。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

/// 配る中身の形（本物の config/git/attributes は crate の外にあり、Nix のビルドの中では読めないので、同じ形をここに書く）
const SOURCE: &str =
    "# claude-harness が置く\n* text=auto eol=lf\n*.bat text eol=crlf\n*.cmd text eol=crlf\n";

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
            "git-attributes-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let home = Home(dir);
        std::fs::write(home.source(), SOURCE).unwrap();
        home
    }

    fn source(&self) -> PathBuf {
        self.0.join("source-attributes")
    }

    fn gitconfig(&self) -> PathBuf {
        self.0.join(".gitconfig")
    }

    fn attributes(&self) -> PathBuf {
        self.0.join(".config").join("git").join("attributes")
    }

    /// 利用者の git の設定と環境を見せない command。env は足す環境変数
    fn command(&self, program: &str, env: &[(&str, &Path)]) -> Command {
        let mut cmd = Command::new(program);
        for (key, _) in std::env::vars_os() {
            let key = key.to_string_lossy().into_owned();
            if key.starts_with("GIT_") || key == "XDG_CONFIG_HOME" {
                cmd.env_remove(key);
            }
        }
        cmd.env("GIT_CONFIG_GLOBAL", self.gitconfig())
            .env("HOME", &self.0)
            .env("USERPROFILE", &self.0);
        if !env.iter().any(|(key, _)| *key == "GIT_CONFIG_SYSTEM") {
            cmd.env("GIT_CONFIG_NOSYSTEM", "1");
        }
        for (key, value) in env {
            cmd.env(key, value);
        }
        cmd
    }

    fn git(&self) -> Command {
        self.command("git", &[])
    }

    fn run_with(&self, env: &[(&str, &Path)]) -> Output {
        self.command(env!("CARGO_BIN_EXE_claude-setup"), env)
            .args(["git-attributes", "--from"])
            .arg(self.source())
            .output()
            .unwrap()
    }

    fn run(&self) -> Output {
        self.run_with(&[])
    }

    fn set_global(&self, key: &str, value: &str) {
        let status = self
            .git()
            .args(["config", "--global", key, value])
            .status()
            .unwrap();
        assert!(status.success());
    }
}

fn said(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn the_source_is_placed_where_git_reads_global_attributes() {
    let home = Home::new();
    let out = home.run();
    assert!(out.status.success(), "{}", said(&out));
    assert_eq!(std::fs::read_to_string(home.attributes()).unwrap(), SOURCE);
    // core.attributesFile は書かない（書かなくても git がこの場所を読む）
    let out = home
        .git()
        .args(["config", "--global", "--get", "core.attributesFile"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "{}", said(&out));
}

#[test]
fn xdg_config_home_decides_the_place_like_git() {
    let home = Home::new();
    let xdg = home.0.join("xdg");
    let out = home.run_with(&[("XDG_CONFIG_HOME", &xdg)]);
    assert!(out.status.success(), "{}", said(&out));
    assert_eq!(
        std::fs::read_to_string(xdg.join("git/attributes")).unwrap(),
        SOURCE
    );
    assert!(!home.attributes().exists());
}

#[test]
fn running_twice_keeps_the_same_file() {
    let home = Home::new();
    assert!(home.run().status.success());
    let again = home.run();
    assert!(again.status.success(), "{}", said(&again));
    assert_eq!(std::fs::read_to_string(home.attributes()).unwrap(), SOURCE);
}

#[test]
fn an_older_harness_file_is_updated() {
    // 1 行目が配る中身と同じファイルは、ハーネスが前に置いたもの
    let home = Home::new();
    std::fs::create_dir_all(home.attributes().parent().unwrap()).unwrap();
    std::fs::write(home.attributes(), "# claude-harness が置く\n* text=auto\n").unwrap();
    let out = home.run();
    assert!(out.status.success(), "{}", said(&out));
    assert_eq!(std::fs::read_to_string(home.attributes()).unwrap(), SOURCE);
}

#[test]
fn a_users_own_attributes_file_is_not_replaced() {
    let home = Home::new();
    std::fs::create_dir_all(home.attributes().parent().unwrap()).unwrap();
    std::fs::write(home.attributes(), "*.png binary\n").unwrap();
    let out = home.run();
    assert_eq!(out.status.code(), Some(1), "{}", said(&out));
    assert!(
        said(&out).contains(&home.attributes().display().to_string()),
        "{}",
        said(&out)
    );
    assert_eq!(
        std::fs::read_to_string(home.attributes()).unwrap(),
        "*.png binary\n"
    );
}

// Windows の symlink は作るのに権限が要る
#[cfg(unix)]
#[test]
fn an_attributes_symlink_is_left_to_its_owner() {
    // 中身がハーネスのものでも、リンクの先は持ち主（home-manager など）が管理する
    let home = Home::new();
    std::fs::create_dir_all(home.attributes().parent().unwrap()).unwrap();
    let target = home.0.join("store-attributes");
    std::fs::write(&target, SOURCE).unwrap();
    std::os::unix::fs::symlink(&target, home.attributes()).unwrap();
    let out = home.run();
    assert_eq!(out.status.code(), Some(1), "{}", said(&out));
    assert!(
        std::fs::symlink_metadata(home.attributes())
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn a_global_attributes_file_elsewhere_stops_without_writing() {
    // git はそこを読み、既定の場所は読まないので、置いても効かない
    let home = Home::new();
    home.set_global("core.attributesFile", "/opt/team/attributes");
    let out = home.run();
    assert_eq!(out.status.code(), Some(1), "{}", said(&out));
    assert!(
        said(&out).contains("/opt/team/attributes"),
        "{}",
        said(&out)
    );
    assert!(!home.attributes().exists());
}

#[test]
fn a_global_attributes_file_at_the_same_place_is_fine() {
    let home = Home::new();
    home.set_global("core.attributesFile", "~/.config/git/attributes");
    let out = home.run();
    assert!(out.status.success(), "{}", said(&out));
    assert_eq!(std::fs::read_to_string(home.attributes()).unwrap(), SOURCE);
}

#[test]
fn a_system_attributes_file_elsewhere_stops_without_writing() {
    let home = Home::new();
    let system = home.0.join("system.gitconfig");
    std::fs::write(&system, "[core]\n\tattributesFile = /opt/corp/attributes\n").unwrap();
    let out = home.run_with(&[("GIT_CONFIG_SYSTEM", &system)]);
    assert_eq!(out.status.code(), Some(1), "{}", said(&out));
    assert!(
        said(&out).contains("/opt/corp/attributes"),
        "{}",
        said(&out)
    );
    assert!(!home.attributes().exists());
}

#[test]
fn a_system_attributes_file_is_ignored_when_git_ignores_the_system_file() {
    let home = Home::new();
    let system = home.0.join("system.gitconfig");
    std::fs::write(&system, "[core]\n\tattributesFile = /opt/corp/attributes\n").unwrap();
    let nosystem = Path::new("1");
    let out = home.run_with(&[
        ("GIT_CONFIG_SYSTEM", &system),
        ("GIT_CONFIG_NOSYSTEM", nosystem),
    ]);
    assert!(out.status.success(), "{}", said(&out));
}

#[test]
fn a_missing_source_is_refused() {
    let home = Home::new();
    std::fs::remove_file(home.source()).unwrap();
    let out = home.run();
    assert_eq!(out.status.code(), Some(1), "{}", said(&out));
    assert!(!home.attributes().exists());
}

#[test]
fn wrong_arguments_are_usage_errors() {
    for args in [
        vec!["git-attributes"],
        vec!["git-attributes", "--from"],
        vec!["git-attributes", "--bin", "x"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_claude-setup"))
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
}

#[test]
fn a_repository_without_gitattributes_stores_crlf_files_as_lf() {
    // 置いた attributes を git が読み、.gitattributes の無いリポジトリでも index は LF、*.bat は CRLF で取り出す
    let home = Home::new();
    let out = home.run();
    assert!(out.status.success(), "{}", said(&out));
    let repo = home.0.join("repo");
    let git = |args: &[&str]| {
        let out = home.git().arg("-C").arg(&repo).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", said(&out));
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    std::fs::create_dir_all(&repo).unwrap();
    git(&["init", "-q"]);
    std::fs::write(repo.join("a.txt"), "x\r\ny\r\n").unwrap();
    std::fs::write(repo.join("run.bat"), "echo\n").unwrap();
    git(&["add", "a.txt", "run.bat"]);
    let eol = git(&["ls-files", "--eol", "a.txt", "run.bat"]);
    assert_eq!(eol.lines().count(), 2, "{eol}");
    assert!(eol.lines().all(|line| line.starts_with("i/lf ")), "{eol}");
    std::fs::remove_file(repo.join("run.bat")).unwrap();
    git(&["checkout", "--", "run.bat"]);
    assert_eq!(std::fs::read(repo.join("run.bat")).unwrap(), b"echo\r\n");
}

#[test]
fn a_global_value_at_the_same_place_wins_over_a_system_value_elsewhere() {
    // git は global の値を使い、system の値は読まない
    let home = Home::new();
    home.set_global("core.attributesFile", "~/.config/git/attributes");
    let system = home.0.join("system.gitconfig");
    std::fs::write(&system, "[core]\n\tattributesFile = /opt/corp/attributes\n").unwrap();
    let out = home.run_with(&[("GIT_CONFIG_SYSTEM", &system)]);
    assert!(out.status.success(), "{}", said(&out));
    assert_eq!(std::fs::read_to_string(home.attributes()).unwrap(), SOURCE);
}

#[test]
#[ignore = "RED: まだ直していない"]
fn a_global_attributes_file_spelled_with_another_case_is_the_same_place_on_windows() {
    // Windows のファイルシステムは大文字と小文字を区別しないので、git は同じファイルを読む
    if !cfg!(windows) {
        return;
    }
    let home = Home::new();
    let spelled = home
        .attributes()
        .display()
        .to_string()
        .replace("attributes", "ATTRIBUTES");
    home.set_global("core.attributesFile", &spelled);
    let out = home.run();
    assert!(out.status.success(), "{}", said(&out));
}

#[test]
#[ignore = "RED: まだ直していない"]
fn the_same_content_is_left_untouched() {
    // 中身が同じなら書き直さない（読むだけの属性も、更新の時刻も変えない）
    let home = Home::new();
    assert!(home.run().status.success());
    let mut perms = std::fs::metadata(home.attributes()).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(home.attributes(), perms).unwrap();
    let out = home.run();
    assert!(out.status.success(), "{}", said(&out));
    let after = std::fs::metadata(home.attributes()).unwrap();
    assert!(after.permissions().readonly());
    let mut perms = after.permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(home.attributes(), perms).unwrap();
}
