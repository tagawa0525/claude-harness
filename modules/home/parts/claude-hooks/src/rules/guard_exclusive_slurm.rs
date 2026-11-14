//! guard-exclusive-slurm: 排他の Slurm のジョブが実行中は、計算ノード（r995）の全コアを使うコマンドを止める
//!
//! 排他のジョブ（時間を記録する測定）の間に r995 で全コアを使うと、その時間が乱れて再測定になる
//! （docs/adr/0041）。squeue で OverSubscribe が NO の実行中のジョブがあるときだけ、次を止める。
//! r995 の上で走るもの（このホストが r995 か、ssh の接続先が r995。ホストが分からなければ r995 とみなす）:
//! `cargo build|check|test|clippy|run|bench`（alias `b|c|t|r`）、`dvc repro`、`bin/` と `target/release|debug/` の
//! `xs|lattice|core|chain`、`nix build`、`nix flake check`（`nix develop|shell -c` で包んでも）。1 つの CPU に限る `taskset` で
//! 包めば通す（`-j 1` は限りにならない）。何を実行するか決められないコマンドも止める。
//! ほかのホストで走るもの: 手元のコアは r995 を乱さないので、r995 に投げられるビルドだけを止める。`nix build`、
//! `nix flake check` は taskset で包んでも止め（ビルドは r995 で走る）、`--builders ''` か `--builders @/dev/null` で
//! 手元でビルドするものは通す。
//! rebuild（`nix-rebuild rebuild|update`、shell の関数 `rebuild`・`update`、ビルドする `nixos-rebuild`、
//! `home-manager switch|build`）は taskset で包んでも止める。ただし r995 のほかでは、手元でビルドする
//! `nix-rebuild rebuild|update --local`（nixfiles の nix-rebuild.sh。既定のビルドは r995 に投げる）は通す。
//! `run-checks.sh` と `run-python-tests.sh` は対象にしない（排他のジョブ中は自分で 1 コアに限る）。
//! squeue が無いホスト、squeue の失敗、実行中のジョブが無い場合は何もしない（ラップトップの squeue は r995 の
//! 待ち行列を見る）。
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

/// r995 に投げられうる nix のビルド（`nix build`、`nix flake check`、サブコマンドを決められない nix）か。ほかのホストの
/// ビルドは nix-daemon が r995 に投げるので、taskset は見ない
fn is_nix_build(cmd: &Cmd) -> bool {
    if basename(cmd.program()) != "nix" {
        return false;
    }
    match global_subcommand(&cmd.args) {
        Subcommand::Unknown => true,
        Subcommand::At(i) => {
            let first = cmd.args[i].text.as_str();
            let second = cmd.args.get(i + 1).map(|a| a.text.as_str());
            first == "build" || (first == "flake" && second == Some("check"))
        }
        _ => false,
    }
}

/// `--builders ''` か `--builders @/dev/null`（空のファイルを読ませる）で、リモートのビルダーを使わないか
fn without_remote_builders(cmd: &Cmd) -> bool {
    cmd.args
        .windows(2)
        .any(|w| w[0].text == "--builders" && matches!(w[1].text.as_str(), "" | "@/dev/null"))
}

/// ビルドする nixos-rebuild のサブコマンド（dry-build は評価だけ、list-generations・edit・repl はビルドしない）
const NIXOS_REBUILD_BUILDS: &[&str] = &[
    "switch",
    "boot",
    "test",
    "build",
    "dry-activate",
    "build-vm",
    "build-vm-with-bootloader",
    "build-image",
];

/// rebuild（NixOS か home-manager の構成を作り直す）か。ビルドは nix-daemon の中で走り、クライアントの
/// taskset を引き継がないので、1 コアに限っても全コアを使う（ADR-0041 の「## 変更」）
fn is_rebuild(cmd: &Cmd) -> bool {
    let has_arg = |names: &[&str]| cmd.args.iter().any(|a| names.contains(&a.text.as_str()));
    let program = cmd.program();
    // modules/home/parts/shell.nix の関数は引数を nix-rebuild のサブコマンドの後に渡す。関数はパスで呼べないので、
    // パスで呼ぶ同じ名前のスクリプトは別のもの
    if matches!(program, "rebuild" | "update") {
        return true;
    }
    match basename(program) {
        // nix-rebuild.sh は最初の引数だけでサブコマンドを選ぶ
        "nix-rebuild" | "nix-rebuild.sh" => cmd
            .args
            .first()
            .is_some_and(|a| matches!(a.text.as_str(), "rebuild" | "update")),
        // 値を取るオプション（`--flake`）の後でもサブコマンドを拾う。値と同じ語は普通は書かない。
        // `--rollback` は前の世代に切り替えるだけ、`--help` は使い方を出すだけでビルドしない
        "nixos-rebuild" => {
            has_arg(NIXOS_REBUILD_BUILDS) && !has_arg(&["--rollback", "--help", "-h"])
        }
        "home-manager" => has_arg(&["switch", "build"]) && !has_arg(&["--help", "-h"]),
        _ => false,
    }
}

/// 手元でビルドする rebuild（`nix-rebuild rebuild|update ... --local`。nix-rebuild.sh は `--local` をサブコマンドの後で
/// だけ受ける）か。shell の関数 `rebuild --local` は数えない（前の版の関数は引数を捨て、それを定義したまま走っている
/// シェルでは --local が落ちて r995 に投げる）
fn is_local_rebuild(cmd: &Cmd) -> bool {
    matches!(basename(cmd.program()), "nix-rebuild" | "nix-rebuild.sh")
        && cmd.args.iter().skip(1).any(|a| a.text == "--local")
}

/// 名前が r995 でないか。ドメインの付いた名前と大文字も r995 とみなす
fn names_off_r995(host: &str) -> bool {
    let short = host.trim().split('.').next().unwrap_or_default();
    !short.is_empty() && !short.eq_ignore_ascii_case("r995")
}

/// コマンドの標準出力（成功したときだけ）
fn stdout_of(program: &str, args: &[&str]) -> Option<String> {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// このホストが r995 でないと確かめられたか。r995 の上では手元のビルドも r995 で走るので、`hostname` が失敗したときと
/// 同じく偽にする（止める側に倒す）
fn off_r995() -> bool {
    stdout_of("hostname", &[]).is_some_and(|host| names_off_r995(&host))
}

/// ssh の接続先が r995 でないと確かめられたか。`~/.ssh/config` の別名（Host）は `ssh -G`（つながずに設定を読む）で
/// HostName に解く。解けなければ止める側に倒す
fn destination_off_r995(destination: &str) -> bool {
    stdout_of("ssh", &["-G", destination]).is_some_and(|config| {
        config
            .lines()
            .find_map(|line| line.strip_prefix("hostname "))
            .is_some_and(names_off_r995)
    })
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
        // squeue が無いホスト（Slurm に投げないホスト）。判定の材料が無いだけで、環境の不具合ではない
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
        let commands = shell.commands();
        let rebuilds: Vec<&Cmd> = commands.iter().filter(|c| is_rebuild(c)).collect();
        if rebuilds.is_empty() && !commands.iter().any(|c| is_heavy(c) || is_nix_build(c)) {
            return Vec::new();
        }
        let Some((name, jobid)) = running_exclusive_job() else {
            return Vec::new();
        };
        // 走るホスト（ssh の先なら接続先）が r995 でないか。ホストは排他のジョブがあるときだけ見る
        let off = off_r995();
        let runs_off = |c: &Cmd| match shell.ssh_destination(c) {
            Some(destination) => destination_off_r995(destination),
            None => off,
        };
        // r995 の上では全コアを使うものを止め、ほかのホストでは r995 に投げられるビルドだけを止める
        let heavy_on_node = commands.iter().any(|c| is_heavy(c) && !runs_off(c));
        let build_sent = commands
            .iter()
            .any(|c| is_nix_build(c) && runs_off(c) && !without_remote_builders(c));
        let blocking: Vec<&&Cmd> = rebuilds
            .iter()
            .filter(|c| !(is_local_rebuild(c) && runs_off(c)))
            .collect();
        let rebuild = !blocking.is_empty();
        if !rebuild && !heavy_on_node && !build_sent {
            return Vec::new();
        }
        let local_way = off && blocking.iter().all(|c| shell.ssh_destination(c).is_none());
        let fix = if rebuild && local_way {
            "rebuild のビルドは r995 に投げられ（nix-daemon の中で走り taskset も効かない）、ジョブが終わるまで待つか、nix-rebuild rebuild|update --local（関数の rebuild、update でなく）で手元でビルドしてください。"
        } else if rebuild {
            "rebuild のビルドは nix-daemon の中で走り taskset が効かないので、ジョブが終わるまで待ってください。"
        } else if heavy_on_node {
            "taskset で 1 コアに限る、終わるまで待つ、または slurm-run.sh で投げてください。"
        } else {
            "このホストの nix のビルドは r995 に投げられ（nix-daemon の中で走り taskset も効かない）、ジョブが終わるまで待つか、--builders '' --max-jobs auto を付けて手元でビルドしてください。"
        };
        vec![Finding::Deny(format!(
            "排他の Slurm のジョブ {name}（{jobid}）が実行中で、その時間の記録を乱します。{fix}"
        ))]
    }
}
