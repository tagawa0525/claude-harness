//! `check-each-commit.sh <base> -- <コマンド…>`: `<base>..HEAD` の各コミットを一時の worktree にチェックアウトし、検査の
//! コマンドを実行する（ADR-0101。`~/.claude/scripts/check-each-commit.sh` が呼ぶ）。
//!
//! 引数、出力の行、終了コードの意味は `claude/scripts/check-each-commit.sh` の先頭に書く。ここには、その形を作る流れを置く。
//!
//! 中断（INT、TERM、HUP）は、走っている子のプロセス（git と検査のコマンド）が終わるのを待ってから後片付けをして 130 で
//! 終える。子には中断を送らない（端末の Ctrl-C は、同じプロセスグループの子にも届く）。待つのは、子が一時の worktree の
//! 中で走っている間に worktree を消さないためと、中断されたコミットの行（検査を終えていない結果）を出さないため。子を
//! 実行する間は [`Cleanup`] の Mutex を持ち、中断を受けたスレッドはそれを取ってから片付ける。

use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// 使い方の誤り（引数、base を解決できない、コミットが無い）
const USAGE: i32 = 2;
/// 実行時の環境の失敗（rev-list、一時のディレクトリ、worktree の作成、checkout、clean、標準出力への書き込み）
const ENVIRONMENT: i32 = 3;
/// 読む側が標準出力を先に閉じた（シェルが SIGPIPE で終わったと読む 128 + 13）。後片付けをしてから終える
const BROKEN_PIPE: i32 = 141;

/// git の hook や GIT_DIR を export したシェルから呼ばれると、一時の worktree の checkout と clean が呼び出し元の作業
/// ツリーに作用して消しうるので、git と検査のコマンドに渡さない。リポジトリと作業ツリーを指す変数だけを消す。
/// GIT_CONFIG_*（safe.directory など）やオブジェクトの置き場は、呼び出し側がリポジトリを読めるようにしている設定なので残す
const REPO_ENV: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_PREFIX",
    "GIT_COMMON_DIR",
];

/// フックを止める: post-checkout などが、使い捨ての worktree で副作用（依存の取得や通知）を出さないようにする
const NO_HOOKS: [&str; 2] = ["-c", "core.hooksPath=/dev/null"];

/// 調べる範囲と検査のコマンド（引数を確かめた後）
#[derive(Debug, PartialEq)]
struct Request<'a> {
    base: &'a str,
    command: &'a [String],
}

/// `<base> -- <コマンド…>` を読む。形が違えば None
fn parse_args(args: &[String]) -> Option<Request<'_>> {
    match args {
        [base, sep, command @ ..] if sep == "--" && !command.is_empty() => {
            Some(Request { base, command })
        }
        _ => None,
    }
}

/// 一時のディレクトリを作る親。Unix は TMPDIR で、空か無ければ /tmp にする（std::env::temp_dir は空の TMPDIR を
/// そのまま返す）。Windows は TMPDIR を読まない（Git Bash の /tmp の形のパスは
/// ネイティブの exe が読めない）ので、std::env::temp_dir（TMP、TEMP）にする
fn temp_root(tmpdir: Option<&OsStr>) -> PathBuf {
    if cfg!(unix) {
        match tmpdir {
            Some(dir) if !dir.is_empty() => PathBuf::from(dir),
            _ => PathBuf::from("/tmp"),
        }
    } else {
        std::env::temp_dir()
    }
}

/// 検査のコマンドを起動できなかったときの終了コード。シェルと同じく、見つからなければ 127、ほか（実行の権限が無いなど）は 126
fn spawn_failure_code(kind: std::io::ErrorKind) -> i32 {
    if kind == std::io::ErrorKind::NotFound {
        127
    } else {
        126
    }
}

/// 終わりに消す一時のディレクトリ（worktree はその下の wt）
struct Temp {
    dir: PathBuf,
    wt: PathBuf,
}

impl Temp {
    /// 一時の worktree を消し、一時のディレクトリを消し、worktree の登録を消す。どこかで失敗しても続ける
    fn remove(&self) {
        // 検査が書き込み不可のディレクトリ（Go のモジュールキャッシュなど）を残すと、消すのに失敗する
        make_writable(&self.dir);
        let _ = git(&["worktree", "remove", "--force"])
            .arg(&self.wt)
            .stderr(Stdio::null())
            .status();
        let _ = std::fs::remove_dir_all(&self.dir);
        let _ = git(&["worktree", "prune"]).status();
        if std::fs::symlink_metadata(&self.dir).is_ok() {
            eprintln!("一時ディレクトリを消せませんでした: {}", self.dir.display());
        }
    }
}

/// 中断を受けたスレッドと分け合う、後片付けの対象。子を実行する間は Mutex を持つ（[`Cleanup::child`]）
#[derive(Default)]
struct Cleanup {
    temp: Mutex<Option<Temp>>,
    interrupted: AtomicBool,
}

impl Cleanup {
    fn lock(&self) -> MutexGuard<'_, Option<Temp>> {
        self.temp.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 片付ける（2 回目からは何もしない）。片付ける間も Mutex を持ち、途中で中断されても、中断を受けたスレッドは
    /// 片付け終わるのを待つ
    fn remove(&self) {
        let mut guard = self.lock();
        if let Some(temp) = guard.take() {
            temp.remove();
        }
    }

    /// 中断を受けたスレッドが呼ぶ。走っている子が終わるまで待ってから片付ける
    fn on_interrupt(&self) {
        self.interrupted.store(true, Ordering::SeqCst);
        self.remove();
    }

    /// 子を実行する。中断された後なら、実行せずに（実行した後なら、結果を使わずに）止まり、中断を受けたスレッドが
    /// 片付けて終えるのを待つ
    fn child<T>(&self, run: impl FnOnce() -> T) -> T {
        let guard = self.lock();
        if self.interrupted.load(Ordering::SeqCst) {
            wait_for_exit(guard);
        }
        let result = run();
        if self.interrupted.load(Ordering::SeqCst) {
            wait_for_exit(guard);
        }
        result
    }
}

/// Mutex を放し、中断を受けたスレッドが片付けてプロセスを終えるのを待つ
fn wait_for_exit(guard: MutexGuard<'_, Option<Temp>>) -> ! {
    drop(guard);
    loop {
        std::thread::park();
    }
}

/// git のコマンド（呼び出し元のリポジトリを指す変数を消す）
fn git(args: &[&str]) -> Command {
    let mut cmd = Command::new("git");
    cmd.args(args).stdin(Stdio::null());
    for name in REPO_ENV {
        cmd.env_remove(name);
    }
    cmd
}

/// `chmod -R u+w`（symlink はたどらない。書き込める項目の権限は変えない）。失敗しても続ける
fn make_writable(path: &Path) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    if meta.file_type().is_symlink() {
        return;
    }
    let mut perms = meta.permissions();
    #[cfg(unix)]
    let writable = {
        use std::os::unix::fs::PermissionsExt;
        let writable = perms.mode() & 0o200 != 0;
        perms.set_mode(perms.mode() | 0o200);
        writable
    };
    #[cfg(not(unix))]
    let writable = {
        let writable = !perms.readonly();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        writable
    };
    if !writable {
        let _ = std::fs::set_permissions(path, perms);
    }
    if meta.is_dir()
        && let Ok(entries) = std::fs::read_dir(path)
    {
        for entry in entries.flatten() {
            make_writable(&entry.path());
        }
    }
}

/// 一時のディレクトリを、mktemp -d と同じく持ち主だけが読める形で、ほかと重ならない名前で作る
fn make_temp_dir(root: &Path) -> std::io::Result<PathBuf> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let mut last = None;
    for n in 0..100u32 {
        let dir = root.join(format!(
            "check-each-commit.{}-{nanos}-{n}",
            std::process::id()
        ));
        #[cfg_attr(not(unix), allow(unused_mut))]
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("名前を選べない")))
}

/// git を実行し、成功したら標準出力（末尾の改行を除く）を返す
fn git_output(cleanup: &Cleanup, args: &[&str]) -> Option<String> {
    let out = cleanup
        .child(|| git(args).stderr(Stdio::inherit()).output())
        .ok()?;
    out.status.success().then(|| {
        String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string()
    })
}

fn status_ok(cleanup: &Cleanup, cmd: &mut Command) -> bool {
    cleanup
        .child(|| cmd.status())
        .is_ok_and(|status| status.success())
}

pub fn main(args: &[String]) -> i32 {
    let Some(req) = parse_args(args) else {
        eprintln!("Usage: check-each-commit.sh <base> -- <コマンド…>");
        return USAGE;
    };
    crate::interrupt::broken_pipe_as_error();
    let cleanup = Arc::new(Cleanup::default());
    let shared = Arc::clone(&cleanup);
    if let Err(e) = crate::interrupt::cleanup_on_interrupt(move || shared.on_interrupt()) {
        eprintln!("ERROR: 中断のときの後片付けを登録できない（一時の worktree を消せない）: {e}");
        return ENVIRONMENT;
    }
    let code = run(&req, &cleanup);
    cleanup.remove();
    code
}

fn run(req: &Request, cleanup: &Cleanup) -> i32 {
    let base = req.base;
    let resolved = status_ok(
        cleanup,
        git(&["rev-parse", "--verify", "-q", &format!("{base}^{{commit}}")]).stdout(Stdio::null()),
    );
    if !resolved {
        eprintln!("base を解決できません: {base}");
        return USAGE;
    }
    let range = format!("{base}..HEAD");
    let Some(list) = git_output(cleanup, &["rev-list", "--topo-order", "--reverse", &range]) else {
        eprintln!(
            "git rev-list に失敗しました（shallow clone や壊れたオブジェクトの可能性）: {range}"
        );
        return ENVIRONMENT;
    };
    let commits: Vec<&str> = list.lines().filter(|l| !l.is_empty()).collect();
    if commits.is_empty() {
        eprintln!("{range} にコミットがありません");
        return USAGE;
    }

    let root = temp_root(std::env::var_os("TMPDIR").as_deref());
    let dir = match make_temp_dir(&root) {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("一時ディレクトリを作れません: {}（{e}）", root.display());
            return ENVIRONMENT;
        }
    };
    let wt = dir.join("wt");
    *cleanup.lock() = Some(Temp {
        dir,
        wt: wt.clone(),
    });

    let added = status_ok(
        cleanup,
        git(&NO_HOOKS)
            .args(["worktree", "add", "-q", "--detach"])
            .arg(&wt)
            .arg(commits[0]),
    );
    if !added {
        return ENVIRONMENT;
    }

    let mut rc = 0;
    for sha in commits {
        // 前の検査が書き込み不可のディレクトリを残すと、checkout がその配下の変更で失敗する
        make_writable(&wt);
        let checked_out = status_ok(
            cleanup,
            git(&NO_HOOKS)
                .arg("-C")
                .arg(&wt)
                .args(["checkout", "-q", "--detach", sha]),
        );
        if !checked_out {
            return ENVIRONMENT;
        }
        // 検査の副産物（ビルド結果など）が次のコミットに残らないようにする
        let cleaned = status_ok(cleanup, git(&["-C"]).arg(&wt).args(["clean", "-qffdx"]));
        if !cleaned {
            eprintln!("git clean に失敗したので {sha} を検査しません");
            return ENVIRONMENT;
        }
        let status = cleanup.child(|| inspect(req.command, &wt));
        if status != 0 {
            rc = 1;
        }
        let short = git_output(cleanup, &["rev-parse", "--short", sha]).unwrap_or_default();
        let subject = git_output(cleanup, &["log", "-1", "--format=%s", sha]).unwrap_or_default();
        if let Err(e) = writeln!(std::io::stdout(), "{short} {subject} exit={status}") {
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                return BROKEN_PIPE;
            }
            eprintln!("標準出力に書けません: {e}");
            return ENVIRONMENT;
        }
    }
    rc
}

/// 検査のコマンドを worktree の根で実行し、終了コードを返す。出力は標準エラーに回し（標準出力は 1 コミット 1 行に
/// する）、標準入力を渡さない（パイプラインやエージェントのシェルの入力を食わない）
fn inspect(command: &[String], wt: &Path) -> i32 {
    // Windows では、コマンドを呼んだ Git Bash の sh を通して起動する。CreateProcess は .sh と .cmd を起動せず、相対の
    // パス（./scripts/check.sh）を worktree でなく呼んだ側の作業ディレクトリから引く。ネイティブの exe から起動された
    // MSYS のプログラムは、引数のワイルドカードを自分で展開するので、MSYS の noglob で止める（呼んだ Git Bash は、
    // 引数を展開した後で渡している）
    #[cfg(windows)]
    let (mut cmd, program) = {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "exec \"$@\"", "check-each-commit.sh"]);
        cmd.args(command);
        let msys = std::env::var("MSYS").unwrap_or_default();
        cmd.env("MSYS", format!("{msys} noglob").trim_start());
        (cmd, "sh")
    };
    #[cfg(not(windows))]
    let (mut cmd, program) = {
        let mut cmd = Command::new(&command[0]);
        cmd.args(&command[1..]);
        (cmd, command[0].as_str())
    };
    cmd.current_dir(wt)
        .stdin(Stdio::null())
        .stdout(std::io::stderr());
    for name in REPO_ENV {
        cmd.env_remove(name);
    }
    match cmd.status() {
        Ok(status) => crate::child::exit_code(status),
        Err(e) => {
            eprintln!("check-each-commit.sh: {program}: {e}");
            spawn_failure_code(e.kind())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_base_the_separator_and_a_command_are_read() {
        let args = strings(&["main", "--", "cargo", "build"]);
        let req = parse_args(&args).unwrap();
        assert_eq!(req.base, "main");
        assert_eq!(req.command, &strings(&["cargo", "build"])[..]);
    }

    #[test]
    fn arguments_without_the_separator_or_a_command_are_a_usage_error() {
        assert_eq!(parse_args(&strings(&["main", "true"])), None);
        assert_eq!(parse_args(&strings(&["main", "x", "true"])), None);
        assert_eq!(parse_args(&strings(&["main", "--"])), None);
        assert_eq!(parse_args(&strings(&["main"])), None);
        assert_eq!(parse_args(&[]), None);
    }

    #[test]
    fn a_second_separator_is_passed_to_the_command() {
        let args = strings(&["main", "--", "--", "x"]);
        assert_eq!(
            parse_args(&args).unwrap().command,
            &strings(&["--", "x"])[..]
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_empty_or_missing_tmpdir_falls_back_to_tmp_like_sh() {
        assert_eq!(temp_root(None), PathBuf::from("/tmp"));
        assert_eq!(temp_root(Some(OsStr::new(""))), PathBuf::from("/tmp"));
        assert_eq!(
            temp_root(Some(OsStr::new("/var/t"))),
            PathBuf::from("/var/t")
        );
    }

    #[test]
    fn a_command_that_cannot_be_started_gives_the_codes_of_sh() {
        assert_eq!(spawn_failure_code(std::io::ErrorKind::NotFound), 127);
        assert_eq!(
            spawn_failure_code(std::io::ErrorKind::PermissionDenied),
            126
        );
    }
}
