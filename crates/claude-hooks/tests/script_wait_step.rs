//! `claude-hooks script wait-step` が、/delegate の担当の待ちの 1 ステップを刻み、終わり、1 ステップ、待ちの線を見分けて、
//! 次の行動を示すことを確かめる（`~/.claude/scripts/wait-step.sh` が呼ぶ）。sh から呼ぶ配備の確認は
//! `claude/tests/scripts/wait-step.sh`

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

    /// 1 ステップを step 秒、待ちの線を limit 秒にして、壁の時計で呼び、(終了コード, 標準出力と標準エラー)
    fn wait(&self, step: u64, limit: u64, args: &[&str]) -> (i32, String) {
        self.wait_in(&self.root.join("tmp"), None, step, limit, args)
    }

    /// wait と同じだが、時刻 now（UNIX の秒）から始めた時計で呼ぶ。呼び出しは眠らずに 1 秒ずつ時計を進めるので、
    /// 結果は壁の時計の進み（負荷での遅れ、秒の切り捨て）によらない
    fn wait_at(&self, now: u64, step: u64, limit: u64, args: &[&str]) -> (i32, String) {
        self.wait_in(&self.root.join("tmp"), Some(now), step, limit, args)
    }

    fn wait_in(
        &self,
        tmp: &Path,
        now: Option<u64>,
        step: u64,
        limit: u64,
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
            .env("WAIT_STEP_LIMIT", limit.to_string())
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
#[ignore = "RED: 時計を待ち始めから測り、親に送っても数え直さない"]
fn wait_is_printed_with_exit_2_after_a_step_with_the_next_action() {
    let d = Dir::new();
    let log = d.file("job.out", "");
    let (code, out) = d.wait(1, 60, &["/wt/a", &log]);
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("WAIT: 親に最後に送ってから 0 分"), "{out}");
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
#[ignore = "RED: 時計を待ち始めから測り、親に送っても数え直さない"]
fn an_injected_time_takes_the_steps_without_sleeping() {
    // 時刻を渡すと、1 ステップの 270 秒を眠らずに進め、経過はその時計で数える
    let d = Dir::new();
    let log = d.file("job.out", "");
    let start = Instant::now();
    let (code, out) = d.wait_at(1000, 270, 3300, &["/wt/fake", &log]);
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("WAIT: 親に最後に送ってから 4 分"), "{out}");
    // 1 ステップの 270 秒より十分に短い（負荷の下でも起動と 270 回の見回りは収まる）
    assert!(start.elapsed() < Duration::from_secs(60), "{out}");
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
#[ignore = "RED: 時計を待ち始めから測り、親に送っても数え直さない"]
fn past_the_limit_the_wait_ends_with_the_next_action_and_removes_the_clock() {
    let d = Dir::new();
    let log = d.file("job.out", "");
    // 100 秒に待ち始め、102 秒に WAIT で返る。続けて呼ぶと、線の 103 秒に END
    d.wait_at(100, 2, 3, &["/wt/c", &log]);
    let (code, out) = d.wait_at(102, 2, 3, &["/wt/c", &log]);
    assert_eq!(code, 3, "{out}");
    assert!(
        out.contains("END: 親に最後に送ってから 0 分を超えた"),
        "{out}"
    );
    assert!(out.contains("NEXT: 待つのをやめる"), "{out}");
    assert!(
        out.contains("投げたジョブと、その background の slurm-run.sh は止めない"),
        "{out}"
    );
    assert!(out.contains("slurm-wait.sh で番号から引き継ぐ"), "{out}");
    assert!(out.contains("報告の「続きの作業」に書き"), "{out}");
    assert!(
        out.contains("~/.claude/scripts/wait-step.sh --end /wt/c"),
        "{out}"
    );
    assert!(out.contains("SubagentHandback で報告して終える"), "{out}");
    assert!(!out.contains("REPORT"), "{out}");
    // 終える担当の時計を消すので、同じ worktree の次の担当は今から数える
    assert_eq!(d.wait_at(103, 2, 3, &["/wt/c", &log]).0, 2);
}

/// hook の restart-wait-clock に、担当 agent の tool_name の呼び出し（tool_input は input）を掛ける
fn hook(d: &Dir, agent: &str, tool_name: &str, input: serde_json::Value) {
    use std::io::Write;
    let json = serde_json::json!({
        "tool_name": tool_name,
        "tool_input": input,
        "cwd": d.root,
        "agent_id": agent,
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_claude-hooks"))
        .args(["pre-tool-use", "--rule", "restart-wait-clock"])
        .env("TMPDIR", d.root.join("tmp"))
        .env("TMP", d.root.join("tmp"))
        .env("TEMP", d.root.join("tmp"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(json.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    // ゲートではないので、何も出さない
    assert!(
        out.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// 状態のファイルの、親に最後に送った時刻
fn reported(d: &Dir) -> u64 {
    let dir = d.root.join("tmp").join("claude-wait-step");
    let state = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.is_file())
        .unwrap();
    std::fs::read_to_string(state)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
#[ignore = "RED: 時計を待ち始めから測り、親に送っても数え直さない"]
fn a_message_of_the_agent_to_the_parent_restarts_its_clock() {
    let d = Dir::new();
    let log = d.file("job.out", "");
    // sh の文字列の中の `\` は消えるので、Windows のパスも `/` で区切って書く
    let wt = d.root.join("wt").display().to_string().replace('\\', "/");
    let log = log.replace('\\', "/");
    hook(
        &d,
        "a1",
        "Bash",
        serde_json::json!({"command": format!("~/.claude/scripts/wait-step.sh {wt} {log}")}),
    );
    d.wait_at(100, 1, 3300, &[&wt, &log]);
    assert_eq!(reported(&d), 100);
    // ほかの担当の送信と、親でない宛先は数え直さない
    hook(
        &d,
        "a2",
        "SendMessage",
        serde_json::json!({"to": "main", "message": "x"}),
    );
    hook(
        &d,
        "a1",
        "SendMessage",
        serde_json::json!({"to": "code-review", "message": "x"}),
    );
    assert_eq!(reported(&d), 100);
    hook(
        &d,
        "a1",
        "SendMessage",
        serde_json::json!({"to": "main", "message": "x"}),
    );
    let restarted = reported(&d);
    assert!(restarted > 1_000_000, "{restarted}");
    hook(
        &d,
        "a1",
        "SubagentHandback",
        serde_json::json!({"message": "x"}),
    );
    assert!(reported(&d) >= restarted);
}

#[test]
#[ignore = "RED: 時計を待ち始めから測り、親に送っても数え直さない"]
fn the_default_limit_is_55_minutes_from_the_start_of_the_wait() {
    let d = Dir::new();
    let log = d.file("job.out", "");
    let wait = |now: u64| {
        let out = Command::new(env!("CARGO_BIN_EXE_claude-hooks"))
            .args(["script", "wait-step", "/wt/default", &log])
            .env("TMPDIR", d.root.join("tmp"))
            .env("TMP", d.root.join("tmp"))
            .env("TEMP", d.root.join("tmp"))
            .env("WAIT_STEP_NOW", now.to_string())
            .env("WAIT_STEP_SECONDS", "270")
            .env_remove("WAIT_STEP_LIMIT")
            .output()
            .unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8(out.stdout).unwrap(),
        )
    };
    // 0 秒に待ち始め、270 秒ごとに呼び直す。12 回目は 2970〜3240 秒で WAIT、13 回目は 3300 秒（55 分）で END
    for step in 0..12 {
        let (code, out) = wait(step * 270);
        assert_eq!(code, 2, "{step}: {out}");
    }
    let (code, out) = wait(12 * 270);
    assert_eq!(code, 3, "{out}");
    assert!(
        out.contains("END: 親に最後に送ってから 55 分を超えた（線は 55 分）"),
        "{out}"
    );
}

#[test]
fn the_clock_continues_from_the_start_after_a_done() {
    let d = Dir::new();
    let log = d.file("job.out", "");
    let ended = d.file("ended.out", "[exited with code 0]\n");
    d.wait_at(100, 2, 5, &["/wt/d", &log]);
    d.wait_at(102, 2, 5, &["/wt/d", &log, &ended]);
    // 待ち始めから 4 秒、線の 5 秒の手前。次の 1 ステップで線に届く
    assert_eq!(d.wait_at(104, 2, 5, &["/wt/d", &log]).0, 3);
}

#[test]
fn done_comes_before_the_end_of_the_wait() {
    let d = Dir::new();
    let log = d.file("job.out", "");
    let ended = d.file("ended.out", "[exited with code 0]\n");
    d.wait_at(100, 2, 3, &["/wt/e", &log]);
    // 待ち始めから 4 秒で、線を過ぎている
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
fn a_time_setting_that_is_not_whole_seconds_exits_1_with_its_name() {
    // 読めない値を壁の時計や既定の秒数に替えて続けない（テストが気づかずに壁の時計に頼る）
    let d = Dir::new();
    let log = d.file("job.out", "");
    for (name, value) in [
        ("WAIT_STEP_NOW", "100s"),
        ("WAIT_STEP_SECONDS", "-1"),
        ("WAIT_STEP_LIMIT", "1.5"),
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_claude-hooks"))
            .args(["script", "wait-step", "/wt/bad", &log])
            .env("TMPDIR", d.root.join("tmp"))
            .env("TMP", d.root.join("tmp"))
            .env("TEMP", d.root.join("tmp"))
            .env("WAIT_STEP_NOW", "100")
            .env("WAIT_STEP_SECONDS", "1")
            .env("WAIT_STEP_LIMIT", "60")
            .env(name, value)
            .output()
            .unwrap();
        let err = String::from_utf8(out.stderr).unwrap();
        assert_eq!(out.status.code(), Some(1), "{name}={value}: {err}");
        assert!(err.contains(name) && err.contains(value), "{err}");
    }
}

#[cfg(unix)]
#[test]
fn a_time_setting_that_is_not_utf8_exits_1_with_its_name() {
    use std::os::unix::ffi::OsStrExt;
    let d = Dir::new();
    let log = d.file("job.out", "");
    let out = Command::new(env!("CARGO_BIN_EXE_claude-hooks"))
        .args(["script", "wait-step", "/wt/bad", &log])
        .env("TMPDIR", d.root.join("tmp"))
        .env("WAIT_STEP_NOW", std::ffi::OsStr::from_bytes(b"1\xff"))
        .env("WAIT_STEP_SECONDS", "1")
        .env("WAIT_STEP_LIMIT", "60")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(err.contains("WAIT_STEP_NOW"), "{err}");
}

#[test]
fn an_injected_time_at_the_end_of_the_range_does_not_overflow() {
    let d = Dir::new();
    let log = d.file("job.out", "");
    let (code, out) = d.wait_at(u64::MAX - 1, 270, 3300, &["/wt/max", &log]);
    assert_eq!(code, 2, "{out}");
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
#[ignore = "RED: 時計を待ち始めから測り、親に送っても数え直さない"]
fn a_start_time_later_than_now_does_not_break_the_wait() {
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
    // 101 秒に最後に呼び、待ち始めは 30 秒後
    std::fs::write(&state, "131 101\n").unwrap();
    let (code, out) = d.wait_at(101, 1, 60, &["/wt/clock", &log]);
    assert_eq!(code, 2, "{out}");
    assert!(out.contains("WAIT: 親に最後に送ってから 0 分"), "{out}");
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
fn a_start_time_later_than_now_restarts_the_clock() {
    // 時計が大きく戻ったら、未来の待ち始めの時刻を待たずに数え直す（END が戻った分だけ遅れない）
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
    // 101 秒に最後に呼び、待ち始めは 30 分後
    std::fs::write(&state, "1901 101\n").unwrap();
    // 101 秒から数え直して 103 秒に WAIT。続けて呼ぶと、線の 104 秒に END
    d.wait_at(101, 2, 3, &["/wt/back", &log]);
    assert_eq!(d.wait_at(103, 2, 3, &["/wt/back", &log]).0, 3);
}
