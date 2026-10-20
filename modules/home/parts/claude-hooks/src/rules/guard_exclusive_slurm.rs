//! guard-exclusive-slurm: 排他の Slurm のジョブが走行中は、全コアを使う手元のコマンドを止める
//!
//! 排他のジョブ（時間を記録する測定）の間に手元で全コアを使うと、その時間が乱れて測り直しになる
//! （docs/adr/0041）。squeue で OverSubscribe が NO の走行中のジョブがあるときだけ、対象のコマンドを止める:
//! `cargo build|test|clippy|run|bench`、`dvc repro`、`bin/xs|lattice|core|chain`、`nix build`。
//! `-j 1`（`cargo` のみ）か、1 つの CPU に限る `taskset` で包めば通す。
//! squeue が無いホスト、squeue の失敗、走行中のジョブが無い場合は何もしない（r995 以外では判定の材料が無い）。
//! squeue は対象のコマンドがあるときだけ呼ぶ。エスケープはない。

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Arg, Cmd, Shell};
use crate::unwrap::basename;

pub struct GuardExclusiveSlurm;

const CARGO_HEAVY: &[&str] = &["build", "test", "clippy", "run", "bench"];
const BIN_PROGRAMS: &[&str] = &["xs", "lattice", "core", "chain"];

/// 全コアを使いうる手元のコマンドか。`-j 1` と 1 コアの taskset で限ったものは含めない
fn is_heavy(cmd: &Cmd) -> bool {
    if cmd.pinned_to_one_cpu {
        return false;
    }
    let program = cmd.program();
    let first = first_word(&cmd.args);
    match basename(program) {
        "cargo" => first.is_some_and(|s| CARGO_HEAVY.contains(&s)) && !cargo_single_job(&cmd.args),
        "dvc" => first == Some("repro"),
        "nix" => first == Some("build"),
        name => {
            BIN_PROGRAMS.contains(&name)
                && program
                    .strip_suffix(name)
                    .and_then(|dir| dir.strip_suffix('/'))
                    .is_some_and(|dir| basename(dir) == "bin")
        }
    }
}

/// 値を次の語に取る cargo と nix のグローバルオプション（サブコマンドより前に書ける）
const VALUE_OPTIONS: &[&str] = &[
    "--config",
    "-Z",
    "--extra-experimental-features",
    "--experimental-features",
    "--option",
];

/// サブコマンド（グローバルオプションと、`+nightly` のようなツールチェーンの指定の次の語）
fn first_word(args: &[Arg]) -> Option<&str> {
    let mut words = args.iter().map(|a| a.text.as_str());
    while let Some(w) = words.next() {
        if VALUE_OPTIONS.contains(&w) {
            // `--option` だけは名前と値の 2 語を取る
            words.nth(usize::from(w == "--option"));
        } else if !w.starts_with('-') && !w.starts_with('+') {
            return Some(w);
        }
    }
    None
}

/// `-j 1`、`-j1`、`--jobs 1`、`--jobs=1`
fn cargo_single_job(args: &[Arg]) -> bool {
    let words: Vec<&str> = args.iter().map(|a| a.text.as_str()).collect();
    // `--` より後は cargo のオプションではない
    let words = words.split(|w| *w == "--").next().unwrap_or_default();
    words.iter().enumerate().any(|(i, w)| {
        matches!(*w, "-j1" | "--jobs=1")
            || (matches!(*w, "-j" | "--jobs") && words.get(i + 1) == Some(&"1"))
    })
}

/// squeue の応答を待つ上限。slurmctld が応答しないホストで、対象のコマンドを何十秒も止めないため
const SQUEUE_TIMEOUT: Duration = Duration::from_secs(5);

/// 走行中の排他のジョブ（名前, ジョブ番号）。squeue が無い、失敗した、時間内に終わらない、排他のジョブが
/// 無いときは None。`-O` の固定幅の列は長い名前がジョブ番号と続いてしまうので、区切りのある `-o` で読む
/// （OverSubscribe は `%h`。排他のジョブは `NO`）
fn running_exclusive_job() -> Option<(String, String)> {
    let mut child = Command::new("squeue")
        .args(["-h", "-t", "R", "-o", "%h|%j|%i"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + SQUEUE_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    if !status.success() {
        return None;
    }
    let mut stdout = String::new();
    child.stdout.take()?.read_to_string(&mut stdout).ok()?;
    stdout.lines().find_map(|line| {
        let (shares, rest) = line.split_once('|')?;
        let (name, jobid) = rest.rsplit_once('|')?;
        (shares.trim() == "NO").then(|| (name.to_string(), jobid.trim().to_string()))
    })
}

impl Rule for GuardExclusiveSlurm {
    fn name(&self) -> &'static str {
        "guard-exclusive-slurm"
    }

    fn check(&self, _input: &Input, shell: &Shell) -> Vec<Finding> {
        if !shell.commands().iter().any(is_heavy) {
            return Vec::new();
        }
        let Some((name, jobid)) = running_exclusive_job() else {
            return Vec::new();
        };
        vec![Finding::Deny(format!(
            "排他の Slurm のジョブ {name}（{jobid}）が走行中で、その時間の記録を乱します。-j 1 と taskset で 1 コアに限る、終わるまで待つ、または slurm-run.sh で投げてください（nixfiles の ADR-0026、ADR-0041）。"
        ))]
    }
}
