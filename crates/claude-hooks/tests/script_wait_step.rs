//! `claude-hooks script wait-step` が、/delegate の担当の待ちの 1 歩を刻み、終わり、1 歩、途中の報告を見分けて、次の行動を
//! 示すことを確かめる（`~/.claude/scripts/wait-step.sh` が呼ぶ）。sh から呼ぶ配備の確認は `claude/tests/scripts/wait-step.sh`

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// テストごとの一時のディレクトリ（状態の置き場と、background の出力のファイル）
struct Dir {
    root: PathBuf,
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Dir {
    fn new() -> Dir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "wait-step-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("tmp")).unwrap();
        Dir { root }
    }

    fn file(&self, name: &str, text: &str) -> String {
        let path = self.root.join(name);
        std::fs::write(&path, text).unwrap();
        path.display().to_string()
    }

    /// 1 歩を step 秒、途中の報告の間隔を report 秒にして、壁の時計で呼び、(終了コード, 標準出力と標準エラー)
    fn wait(&self, step: u64, report: u64, args: &[&str]) -> (i32, String) {
        self.wait_in(&self.root.join("tmp"), None, step, report, args)
    }

    /// wait と同じだが、時刻 now（UNIX の秒）から始めた時計で呼ぶ。呼び出しは眠らずに 1 秒ずつ時計を進めるので、
    /// 結果は壁の時計の進み（負荷での遅れ、秒の切り上げ）によらない
    fn wait_at(&self, now: u64, step: u64, report: u64, args: &[&str]) -> (i32, String) {
        self.wait_in(&self.root.join("tmp"), Some(now), step, report, args)
    }

    fn wait_in(
        &self,
        tmp: &Path,
        now: Option<u64>,
        step: u64,
        report: u64,
        args: &[&str],
    ) -> (i32, String) {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_claude-hooks"));
        if let Some(now) = now {
            cmd.env("WAIT_STEP_NOW", now.to_string());
        }
        let out = cmd
            .args(["script", "wait-step"])
            .args(args)
            // 一時のディレクトリは Unix は TMPDIR、Windows は TMP と TEMP で決まる
            .env("TMPDIR", tmp)
            .env("TMP", tmp)
            .env("TEMP", tmp)
            .env("WAIT_STEP_SECONDS", step.to_string())
            .env("WAIT_STEP_REPORT", report.to_string())
            .output()
            .unwrap();
        (
            out.status.code().unwrap(),
            format!(
                "{}{}",
                String::from_utf8(out.stdout).unwrap(),
                String::from_utf8(out.stderr).unwrap()
            ),
        )
    }
}

#[test]
fn wait_is_printed_with_exit_2_after_a_step_with_the_next_action() {
    let d = Dir::new();
    let log = d.file("job.out", "");
    let (code, out) = d.wait(1, 60, &["/wt/a", &log]);
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("WAIT: 途中の報告から 0 分"), "{out}");
    assert!(
        out.contains("NEXT: ターンを終えずに、同じ引数でもう一度呼ぶ"),
        "{out}"
    );
    assert!(out.contains("通知（終わりの通知、親の返事）"), "{out}");
    // 通知で待ちが済んだときも、時計を消すコマンドを示す
    assert!(
        out.contains("~/.claude/scripts/wait-step.sh --end /wt/a"),
        "{out}"
    );
}

#[test]
fn an_injected_time_takes_the_steps_without_sleeping() {
    // 時刻を渡すと、1 歩の 270 秒を眠らずに進め、経過はその時計で数える
    let d = Dir::new();
    let log = d.file("job.out", "");
    let start = Instant::now();
    let (code, out) = d.wait_at(1000, 270, 3300, &["/wt/fake", &log]);
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("WAIT: 途中の報告から 4 分"), "{out}");
    assert!(start.elapsed() < Duration::from_secs(5), "{out}");
}

#[test]
fn a_finished_or_killed_output_returns_done_at_once() {
    for end in ["[exited with code 0]", "[killed]"] {
        let d = Dir::new();
        let log = d.file("job.out", "");
        let done = d.file("done.out", &format!("job output\n{end}\n"));
        let start = Instant::now();
        let (code, out) = d.wait(6, 60, &["/wt/b", &log, &done]);
        assert_eq!(code, 0, "{out}");
        assert!(out.contains(&format!("DONE: {done}")), "{out}");
        assert!(start.elapsed() < Duration::from_secs(5));
    }
}

#[test]
fn the_end_mark_counts_only_on_the_last_line() {
    let d = Dir::new();
    let mid = d.file("mid.out", "[exited with code 0]\nstill running\n");
    assert_eq!(d.wait(1, 60, &["/wt/mid", &mid]).0, 2);
}

#[test]
fn done_shows_the_command_with_the_remaining_files_and_the_end_command() {
    let d = Dir::new();
    let log = d.file("job.out", "");
    let fin = d.file("fin.out", "[exited with code 0]\n");
    let (code, out) = d.wait(1, 60, &["/wt/next done", &log, &fin]);
    assert_eq!(code, 0, "{out}");
    // 呼び直すコマンドは、終わったファイルを外し、残りのファイルを並べる（Windows のパスは引用符で囲まれる）
    let again = out
        .split("ターンを終えずに ")
        .nth(1)
        .and_then(|s| s.split(" で呼び直し").next())
        .unwrap_or_default();
    assert!(
        again.starts_with("~/.claude/scripts/wait-step.sh '/wt/next done' "),
        "{out}"
    );
    assert!(
        again.contains("job.out") && !again.contains("fin.out"),
        "{out}"
    );
    assert!(
        out.contains("~/.claude/scripts/wait-step.sh --end '/wt/next done' で待ちを消す"),
        "{out}"
    );
}

#[test]
fn done_of_the_last_file_shows_the_call_without_files() {
    let d = Dir::new();
    let fin = d.file("fin.out", "[killed]\n");
    let (code, out) = d.wait(1, 60, &["/wt/last", &fin]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("出力のファイルの無い待ち"), "{out}");
    assert!(
        out.contains("~/.claude/scripts/wait-step.sh /wt/last で呼び直し"),
        "{out}"
    );
    assert!(
        out.contains("~/.claude/scripts/wait-step.sh --end /wt/last"),
        "{out}"
    );
}

#[test]
fn report_after_the_interval_shows_the_message_to_the_parent_and_restarts_the_clock() {
    let d = Dir::new();
    let log = d.file("job.out", "");
    // 100 秒に待ち始め、102 秒に WAIT で返る。続けて呼ぶと、報告の間隔の 103 秒に REPORT
    d.wait_at(100, 2, 3, &["/wt/c", &log]);
    let (code, out) = d.wait_at(102, 2, 3, &["/wt/c", &log]);
    assert_eq!(code, 3, "{out}");
    assert!(out.contains("REPORT:"), "{out}");
    assert!(out.contains("SendMessage（to: \"main\"）"), "{out}");
    assert!(
        out.contains("<待つもの> の待ちが 55 分を超えた。終わり次第、残りの作業を続ける"),
        "{out}"
    );
    assert!(out.contains("同じ引数でもう一度呼ぶ"), "{out}");
    assert!(out.contains("止めるかは親の返事で決める"), "{out}");
    // 103 秒から数え直したので、次の 1 歩（105 秒）は WAIT
    assert_eq!(d.wait_at(103, 2, 3, &["/wt/c", &log]).0, 2);
}

#[test]
fn the_clock_continues_from_the_report_after_a_done() {
    let d = Dir::new();
    let log = d.file("job.out", "");
    let ended = d.file("ended.out", "[exited with code 0]\n");
    d.wait_at(100, 2, 5, &["/wt/d", &log]);
    d.wait_at(102, 2, 5, &["/wt/d", &log, &ended]);
    // 待ち始めから 4 秒、報告の間隔 5 秒の手前。次の 1 歩で報告の時刻に届く
    assert_eq!(d.wait_at(104, 2, 5, &["/wt/d", &log]).0, 3);
}

#[test]
fn done_comes_before_a_due_report() {
    let d = Dir::new();
    let log = d.file("job.out", "");
    let ended = d.file("ended.out", "[exited with code 0]\n");
    d.wait_at(100, 2, 3, &["/wt/e", &log]);
    // 待ち始めから 4 秒で、報告の時刻を過ぎている
    assert_eq!(d.wait_at(104, 2, 3, &["/wt/e", &log, &ended]).0, 0);
}

#[test]
fn a_gap_longer_than_the_interval_restarts_the_clock() {
    let d = Dir::new();
    let log = d.file("job.out", "");
    d.wait_at(100, 1, 3, &["/wt/f", &log]);
    // 101 秒に返った後、4 秒空ける
    assert_eq!(d.wait_at(105, 1, 3, &["/wt/f", &log]).0, 2);
}

#[test]
fn end_removes_the_clock() {
    let d = Dir::new();
    let log = d.file("job.out", "");
    d.wait_at(100, 2, 3, &["/wt/g", &log]);
    assert_eq!(d.wait_at(102, 2, 3, &["--end", "/wt/g"]).0, 0);
    assert_eq!(d.wait_at(102, 2, 3, &["/wt/g", &log]).0, 2);
}

#[test]
fn clocks_are_kept_per_key_and_the_key_path_is_normalized() {
    let d = Dir::new();
    let log = d.file("job.out", "");
    d.wait_at(100, 2, 3, &["/実験/x", &log]);
    assert_eq!(d.wait_at(102, 2, 3, &["/計算/x", &log]).0, 2);
    d.wait_at(100, 2, 3, &["/wt/n", &log]);
    assert_eq!(d.wait_at(102, 2, 3, &["/wt/n/", &log]).0, 3);
}

#[test]
fn a_missing_output_file_exits_1_with_its_path() {
    let d = Dir::new();
    let none = d.root.join("no-such.out").display().to_string();
    let (code, out) = d.wait(1, 60, &["/wt/h", &none]);
    assert_eq!(code, 1);
    assert!(out.contains("no-such.out"), "{out}");
}

#[cfg(unix)]
#[test]
fn an_unwritable_state_exits_1_with_the_reason() {
    let d = Dir::new();
    let log = d.file("job.out", "");
    let (code, out) = d.wait_in(Path::new("/proc/no-such"), None, 1, 60, &["/wt/i", &log]);
    assert_eq!(code, 1);
    assert!(out.contains("状態を書けません"), "{out}");
}

#[test]
fn usage_errors_exit_1() {
    let d = Dir::new();
    for args in [&[][..], &[""][..], &["--end"][..], &["--end", "a", "b"][..]] {
        let (code, out) = d.wait(1, 60, args);
        assert_eq!(code, 1, "{args:?}");
        assert!(out.contains("Usage"), "{args:?}: {out}");
    }
}

#[test]
fn a_report_time_later_than_now_does_not_break_the_wait() {
    // 時計が戻った（NTP の補正）後も、経過を負にせず WAIT を返す
    let d = Dir::new();
    let log = d.file("job.out", "");
    d.wait_at(100, 1, 60, &["/wt/clock", &log]);
    let dir = d.root.join("tmp").join("claude-wait-step");
    let state = std::fs::read_dir(&dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    // 101 秒に最後に呼び、途中の報告は 30 秒後
    std::fs::write(&state, "131 101\n").unwrap();
    let (code, out) = d.wait_at(101, 1, 60, &["/wt/clock", &log]);
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("WAIT: 途中の報告から 0 分"), "{out}");
}

#[cfg(unix)]
#[test]
fn a_key_reached_through_a_symlink_shares_the_clock() {
    let d = Dir::new();
    let log = d.file("job.out", "");
    let real = d.root.join("real");
    std::fs::create_dir_all(real.join("wt")).unwrap();
    let link = d.root.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    d.wait_at(100, 2, 3, &[&real.join("wt").display().to_string(), &log]);
    assert_eq!(
        d.wait_at(102, 2, 3, &[&link.join("wt").display().to_string(), &log])
            .0,
        3
    );
}

#[test]
fn a_report_time_later_than_now_restarts_the_clock() {
    // 時計が大きく戻ったら、未来の途中の報告の時刻を待たずに数え直す（REPORT が戻った分だけ遅れない）
    let d = Dir::new();
    let log = d.file("job.out", "");
    d.wait_at(100, 1, 3, &["/wt/back", &log]);
    let dir = d.root.join("tmp").join("claude-wait-step");
    let state = std::fs::read_dir(&dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    // 101 秒に最後に呼び、途中の報告は 30 分後
    std::fs::write(&state, "1901 101\n").unwrap();
    // 101 秒から数え直して 103 秒に WAIT。続けて呼ぶと、報告の間隔の 104 秒に REPORT
    d.wait_at(101, 2, 3, &["/wt/back", &log]);
    assert_eq!(d.wait_at(103, 2, 3, &["/wt/back", &log]).0, 3);
}
