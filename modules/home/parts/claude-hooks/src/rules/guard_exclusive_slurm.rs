//! guard-exclusive-slurm: 排他の Slurm のジョブが実行中は、全コアを使う手元のコマンドを止める
//!
//! 排他のジョブ（時間を記録する測定）の間に手元で全コアを使うと、その時間が乱れて再測定になる
//! （docs/adr/0041）。squeue で OverSubscribe が NO の実行中のジョブがあるときだけ、対象のコマンドを止める:
//! `cargo build|check|test|clippy|run|bench`（alias `b|c|t|r`）、`dvc repro`、`bin/` と `target/release|debug/` の
//! `xs|lattice|core|chain`、`nix build`、`nix flake check`（`nix develop|shell -c` で包んでも）。1 つの CPU に限る `taskset` で
//! 包めば通す（`-j 1` は限りにならない）。何を実行するか決められないコマンドも、排他のジョブ中は止める。
//! `run-checks.sh` と `run-python-tests.sh` は対象にしない（排他のジョブ中は自分で 1 コアに限る）。
//! squeue が無いホスト、squeue の失敗、実行中のジョブが無い場合は何もしない（r995 以外では判定の材料が無い）。
//! squeue は対象のコマンドがあるときだけ呼ぶ。エスケープはない。

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Cmd, Shell};
use crate::unwrap::{Subcommand, basename, global_subcommand};

pub struct GuardExclusiveSlurm;

/// 全コアを使いうる cargo のサブコマンド（`b`・`c`・`t`・`r` は build・check・test・run の組み込みの alias）。
/// `-j 1` は限りにならない: rustc の codegen、proc-macro と build.rs、テストの実行、実行するプログラムが
/// 別に複数スレッドで走るので、1 コアに限るのは taskset だけ（ADR-0041）
const CARGO_HEAVY: &[&str] = &[
    "build", "b", "check", "c", "test", "t", "clippy", "run", "r", "bench",
];
const BIN_PROGRAMS: &[&str] = &["xs", "lattice", "core", "chain"];

/// `bin/<名前>`、または cargo の出力 `<target の下>/release|debug/<名前>`（`target/<triple>/release/` や
/// `CARGO_TARGET_DIR` の下を含む。ビルドした直後の実行）か
fn is_project_binary(program: &str, name: &str) -> bool {
    program
        .strip_suffix(name)
        .and_then(|dir| dir.strip_suffix('/'))
        .is_some_and(|dir| matches!(basename(dir), "bin" | "release" | "debug"))
}

/// 全コアを使いうる手元のコマンドか。1 コアの taskset で限ったものは含めない。何を実行するか決められない
/// コマンド（`$CMD test`、`bash -s`、`source x.sh`）と、サブコマンドを決められない cargo・nix・dvc は、
/// 止める側に倒す（ADR-0025）
fn is_heavy(cmd: &Cmd) -> bool {
    if cmd.pinned_to_one_cpu {
        return false;
    }
    if cmd.opaque {
        return true;
    }
    let program = cmd.program();
    let sub = global_subcommand(&cmd.args);
    let name = basename(program);
    if sub == Subcommand::Unknown && matches!(name, "cargo" | "nix" | "dvc") {
        return true;
    }
    let (first, second) = match sub {
        Subcommand::At(i) => (
            Some(cmd.args[i].text.as_str()),
            cmd.args.get(i + 1).map(|a| a.text.as_str()),
        ),
        _ => (None, None),
    };
    match name {
        "cargo" => first.is_some_and(|s| CARGO_HEAVY.contains(&s)),
        "dvc" => first == Some("repro"),
        "nix" => first == Some("build") || (first == Some("flake") && second == Some("check")),
        _ => BIN_PROGRAMS.contains(&name) && is_project_binary(program, name),
    }
}

/// squeue の応答を待つ上限。slurmctld が応答しないホストで、対象のコマンドを何十秒も止めないため
const SQUEUE_TIMEOUT: Duration = Duration::from_secs(5);

/// squeue を判定の材料に使えなかった理由を利用者に見せる（ADR-0033）。黙って許可にしない
fn squeue_unavailable(reason: &str) -> Option<(String, String)> {
    crate::output::notice(format!(
        "claude-hooks: squeue の結果を使えず、排他の Slurm のジョブの有無を確かめられませんでした（{reason}）"
    ));
    None
}

/// 実行中の排他のジョブ（名前, ジョブ番号）。squeue が無い、排他のジョブが無いときは None（理由は見せない）。
/// squeue が失敗した、時間内に終わらない、出力を読めないときも None だが、理由を見せる。`-O` の固定幅の列は
/// 長い名前がジョブ番号と続いてしまうので、区切りのある `-o` で読む（OverSubscribe は `%h`。実機の値は
/// 排他のジョブが `NO`、共有できるジョブが `OK` か `YES`）
fn running_exclusive_job() -> Option<(String, String)> {
    let mut child = match Command::new("squeue")
        .args(["-h", "-t", "R", "-o", "%h|%j|%i"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        // r995 以外のホストには squeue が無い。判定の材料が無いだけで、環境の不具合ではない
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => return squeue_unavailable(&format!("起動できません: {e}")),
    };
    // 出力が多いと、読まない間に squeue がパイプを満たして終われない。別のスレッドで並行して読む
    let mut pipe = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        pipe.read_to_end(&mut bytes).map(|_| bytes)
    });
    let deadline = Instant::now() + SQUEUE_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                // 殺すとパイプが閉じ、読むスレッドも終わる
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let bytes = reader.join();
    let Some(status) = status else {
        return squeue_unavailable("5 秒で終わりません");
    };
    if !status.success() {
        return squeue_unavailable(&format!("終了状態 {status}"));
    }
    let Ok(Ok(bytes)) = bytes else {
        return squeue_unavailable("出力を読めません");
    };
    // 名前が UTF-8 でなくても、OverSubscribe とジョブ番号は読める
    String::from_utf8_lossy(&bytes).lines().find_map(|line| {
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
            "排他の Slurm のジョブ {name}（{jobid}）が実行中で、その時間の記録を乱します。taskset で 1 コアに限る、終わるまで待つ、または slurm-run.sh で投げてください。"
        ))]
    }
}
