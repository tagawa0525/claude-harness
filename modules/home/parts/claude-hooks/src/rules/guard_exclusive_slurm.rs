//! guard-exclusive-slurm: 排他の Slurm のジョブが走行中は、全コアを使う手元のコマンドを止める
//!
//! 排他のジョブ（時間を記録する測定）の間に手元で全コアを使うと、その時間が乱れて測り直しになる
//! （docs/adr/0041）。squeue で OverSubscribe が NO の走行中のジョブがあるときだけ、対象のコマンドを止める:
//! `cargo build|check|test|clippy|run|bench`（alias `b|c|t|r`）、`dvc repro`、`bin/` と `target/release|debug/` の
//! `xs|lattice|core|chain`、`nix build`、`nix flake check`（`nix develop|shell -c` で包んでも）。`-j 1`
//! （`cargo check|clippy` のみ）か、1 つの CPU に限る `taskset` で包めば通す。
//! squeue が無いホスト、squeue の失敗、走行中のジョブが無い場合は何もしない（r995 以外では判定の材料が無い）。
//! squeue は対象のコマンドがあるときだけ呼ぶ。エスケープはない。

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Cmd, Shell};
use crate::unwrap::{basename, env_value, global_subcommand};

pub struct GuardExclusiveSlurm;

/// 全コアを使いうる cargo のサブコマンド（`b`・`c`・`t`・`r` は build・check・test・run の組み込みの alias）
const CARGO_HEAVY: &[&str] = &[
    "build", "b", "check", "c", "test", "t", "clippy", "run", "r", "bench",
];
/// `-j 1` で使うコアを 1 つに限れるサブコマンド。`build` は rustc の codegen が複数スレッドで走り、`test`・
/// `run`・`bench` はテストの実行や実行するプログラムが複数スレッドで走るので、`-j 1` では限れない（ADR-0041）
const CARGO_LIMITED_BY_JOBS: &[&str] = &["check", "c", "clippy"];
const BIN_PROGRAMS: &[&str] = &["xs", "lattice", "core", "chain"];

/// `bin/<名前>`、または cargo の出力 `target/release|debug/<名前>`（ビルドした直後の実行）か
fn is_project_binary(program: &str, name: &str) -> bool {
    let Some(dir) = program
        .strip_suffix(name)
        .and_then(|dir| dir.strip_suffix('/'))
    else {
        return false;
    };
    match basename(dir) {
        "bin" => true,
        "release" | "debug" => dir
            .rsplit_once('/')
            .is_some_and(|(parent, _)| basename(parent) == "target"),
        _ => false,
    }
}

/// 全コアを使いうる手元のコマンドか。1 コアの taskset で限ったものは含めない
fn is_heavy(cmd: &Cmd) -> bool {
    if cmd.pinned_to_one_cpu {
        return false;
    }
    let program = cmd.program();
    let sub = global_subcommand(&cmd.args);
    let first = sub.map(|i| cmd.args[i].text.as_str());
    let second = sub
        .and_then(|i| cmd.args.get(i + 1))
        .map(|a| a.text.as_str());
    match basename(program) {
        "cargo" => {
            first.is_some_and(|s| CARGO_HEAVY.contains(&s))
                && !(first.is_some_and(|s| CARGO_LIMITED_BY_JOBS.contains(&s))
                    && cargo_single_job(cmd))
        }
        "dvc" => first == Some("repro"),
        "nix" => first == Some("build") || (first == Some("flake") && second == Some("check")),
        name => BIN_PROGRAMS.contains(&name) && is_project_binary(program, name),
    }
}

/// `-j 1`、`-j1`、`--jobs 1`、`--jobs=1`（サブコマンドの前でもよい）、または環境変数 `CARGO_BUILD_JOBS=1`
fn cargo_single_job(cmd: &Cmd) -> bool {
    if env_value(&cmd.env, "CARGO_BUILD_JOBS", None) == Some("1") {
        return true;
    }
    let words: Vec<&str> = cmd.args.iter().map(|a| a.text.as_str()).collect();
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
    // 出力が多いと、読まない間に squeue がパイプを満たして終われない。別のスレッドで並行して読む
    let mut pipe = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        pipe.read_to_string(&mut out).map(|_| out)
    });
    let deadline = Instant::now() + SQUEUE_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                // 殺すとパイプが閉じ、読むスレッドも終わる
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let stdout = reader.join().ok()?.ok()?;
    if !status.success() {
        return None;
    }
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
