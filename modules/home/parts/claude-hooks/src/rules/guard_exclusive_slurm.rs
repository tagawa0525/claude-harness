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
    if sub == Subcommand::Unknown && matches!(name, "cargo" | "dvc") {
        return true;
    }
    let first = match sub {
        Subcommand::At(i) => Some(cmd.args[i].text.as_str()),
        _ => None,
    };
    match name {
        "cargo" => first.is_some_and(|s| CARGO_HEAVY.contains(&s)),
        "dvc" => first == Some("repro"),
        _ => BIN_PROGRAMS.contains(&name) && is_project_binary(program, name),
    }
}

/// nix のビルド（`nix build`、`nix flake check`、サブコマンドを決められない nix）か。ビルドは nix-daemon の中で走り
/// （ほかのホストでは計算ノードに投げられ）、クライアントの taskset を引き継がないので、taskset は見ない
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

/// ホストの名前の比べ方（ドメインの前の名前を小文字で）
fn short_name(host: &str) -> String {
    host.trim()
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
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

/// このホストの名前。`hostname` が失敗したら None
fn this_host() -> Option<String> {
    stdout_of("hostname", &[]).filter(|h| !h.trim().is_empty())
}

/// ssh の接続先のホストの名前。`~/.ssh/config` の別名（Host）は `ssh -G`（つながずに設定を読む）で HostName に解く。
/// `localhost` と loopback のアドレスはこのホスト（this）にし、ほかの IP アドレスは名前と比べられないので None
fn destination_host(destination: &str, this: Option<&str>) -> Option<String> {
    let host = stdout_of("ssh", &["-G", destination])?
        .lines()
        .find_map(|line| line.strip_prefix("hostname "))?
        .trim()
        .to_string();
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) if ip.is_loopback() => this.map(str::to_string),
        Ok(_) => None,
        Err(_) if host.eq_ignore_ascii_case("localhost") => this.map(str::to_string),
        Err(_) => Some(host),
    }
}

/// このホストの nix のビルドが nodes のどれかに投げられるか。リモートのビルダーは `$NIX_CONF_DIR/machines`
/// （既定 /etc/nix。NixOS が nix.buildMachines から書く）の各行の先頭の URI（`ssh-ng://<user>@<host>`）。ファイルが
/// 無ければ手元でビルドする。読めなければ投げるとみなす（止める側に倒す）
fn builds_reach(nodes: &[String]) -> bool {
    let dir = std::env::var_os("NIX_CONF_DIR").unwrap_or_else(|| "/etc/nix".into());
    let machines = match std::fs::read_to_string(std::path::Path::new(&dir).join("machines")) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return false,
        Err(_) => return true,
    };
    machines
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|uri| !uri.starts_with('#'))
        .any(|uri| {
            let host = uri.split_once("://").map_or(uri, |(_, rest)| rest);
            let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
            let host = host.split([':', '/']).next().unwrap_or_default();
            nodes.contains(&short_name(host))
        })
}

/// 実行中の排他のジョブ
struct ExclusiveJob {
    name: String,
    id: String,
    /// ジョブのノード（短い名前）。縮めた形（`node[1-3]`）や列の無い出力で読めなければ None
    nodes: Option<Vec<String>>,
}

impl ExclusiveJob {
    /// host（名前が分からなければ None）がジョブのノードか。分からなければノードとみなす（止める側に倒す）
    fn runs_on(&self, host: Option<&str>) -> bool {
        match (host, &self.nodes) {
            (Some(host), Some(nodes)) => nodes.contains(&short_name(host)),
            _ => true,
        }
    }
}

/// squeue の応答を待つ上限。slurmctld が応答しないホストで、対象のコマンドを何十秒も止めないため
const SQUEUE_TIMEOUT: Duration = Duration::from_secs(5);

/// squeue を判定の材料に使えなかった理由を利用者に見せる（ADR-0033）。黙って許可にしない
fn squeue_unavailable(reason: &str) -> Vec<ExclusiveJob> {
    crate::output::notice(format!(
        "claude-hooks: squeue の結果を使えず、排他の Slurm のジョブの有無を確かめられませんでした（{reason}）"
    ));
    Vec::new()
}

/// 実行中の排他のジョブのすべて（計算ノードが複数あれば、ノードごとに走りうる）。squeue が無い、排他のジョブが無いときは None（理由は見せない）。
/// squeue が失敗した、時間内に終わらない、出力を読めないときも None だが、理由を見せる。`-O` の固定幅の列は
/// 長い名前がジョブ番号と続いてしまうので、区切りのある `-o` で読む（OverSubscribe は `%h`。実機の値は
/// 排他のジョブが `NO`、共有できるジョブが `OK` か `YES`）
fn running_exclusive_jobs() -> Vec<ExclusiveJob> {
    let mut child = match Command::new("squeue")
        .args(["-h", "-t", "R", "-o", "%h|%j|%i|%N"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        // squeue が無いホスト（Slurm に投げないホスト）。判定の材料が無いだけで、環境の不具合ではない
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => return squeue_unavailable(&format!("起動できません: {e}")),
    };
    // 出力が多いと、読まない間に squeue がパイプを満たして終われない。別のスレッドで並行して読む
    let Some(mut pipe) = child.stdout.take() else {
        return Vec::new();
    };
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
    // 名前が UTF-8 でなくても、OverSubscribe とジョブ番号とノードは読める。名前に | があっても、ノードと番号は後ろから取る
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|line| {
            let (shares, rest) = line.split_once('|')?;
            if shares.trim() != "NO" {
                return None;
            }
            let (rest, node) = rest.rsplit_once('|')?;
            let (name, id, nodes) = match rest.rsplit_once('|') {
                Some((name, id)) => {
                    let node = node.trim();
                    let nodes = (!node.is_empty() && !node.contains('['))
                        .then(|| node.split(',').map(short_name).collect());
                    (name, id, nodes)
                }
                // ノードの列の無い出力（名前|番号）
                None => (rest, node, None),
            };
            Some(ExclusiveJob {
                name: name.to_string(),
                id: id.trim().to_string(),
                nodes,
            })
        })
        .collect()
}

impl Rule for GuardExclusiveSlurm {
    fn name(&self) -> &'static str {
        "guard-exclusive-slurm"
    }

    fn check(&self, _input: &Input, shell: &Shell) -> Vec<Finding> {
        let commands = shell.commands();
        if !commands
            .iter()
            .any(|c| is_rebuild(c) || is_heavy(c) || is_nix_build(c))
        {
            return Vec::new();
        }
        let jobs = running_exclusive_jobs();
        if jobs.is_empty() {
            return Vec::new();
        }
        // 走るホスト（ssh の先なら接続先）で走る排他のジョブ。ホストは排他のジョブがあるときだけ見て、コマンドごとに
        // 1 度だけ解く（ssh -G を起動し直さない）
        let job_on = |host: Option<&str>| jobs.iter().find(|j| j.runs_on(host));
        let this = this_host();
        let this_job = job_on(this.as_deref());
        // ビルドを投げる先で走る排他のジョブ。このホストは machines で見て、ssh の先のビルダーは読めないので、どれかの
        // ジョブのノードに投げるとみなす
        let reached = std::cell::OnceCell::new();
        let sent_to = |via_ssh: bool| -> Option<&ExclusiveJob> {
            if via_ssh {
                return jobs.first();
            }
            *reached.get_or_init(|| {
                jobs.iter()
                    .find(|j| j.nodes.as_deref().is_none_or(builds_reach))
            })
        };
        // ノードの上では全コアを使うものと nix のビルドを止め、ほかのホストではノードに投げられるビルドだけを止める
        let mut rebuilds: Vec<(&ExclusiveJob, bool)> = Vec::new();
        let (mut sent, mut nix_on_node, mut heavy_on_node) = (None, None, None);
        for c in commands {
            let via_ssh = shell.ssh_destination(c);
            let ssh = via_ssh.is_some();
            let on = via_ssh.map_or(this_job, |d| {
                job_on(destination_host(d, this.as_deref()).as_deref())
            });
            if is_rebuild(c) {
                if let Some(j) =
                    on.or_else(|| (!is_local_rebuild(c)).then(|| sent_to(ssh)).flatten())
                {
                    rebuilds.push((j, ssh));
                }
            } else if is_nix_build(c) {
                match on {
                    Some(j) => nix_on_node = nix_on_node.or(Some(j)),
                    None if !without_remote_builders(c) => sent = sent.or(sent_to(ssh)),
                    None => {}
                }
            } else if is_heavy(c) {
                heavy_on_node = heavy_on_node.or(on);
            }
        }
        let (job, fix) = if let Some(&(j, _)) = rebuilds.first() {
            if this_job.is_none() && rebuilds.iter().all(|&(_, ssh)| !ssh) {
                (
                    j,
                    "rebuild のビルドは計算ノードに投げられ（nix-daemon の中で走り taskset も効かない）、ジョブが終わるまで待つか、nix-rebuild rebuild|update --local（関数の rebuild、update でなく）で手元でビルドしてください。",
                )
            } else {
                (
                    j,
                    "rebuild のビルドは nix-daemon の中で走り taskset が効かないので、ジョブが終わるまで待ってください。",
                )
            }
        } else if let (Some(j), None) = (sent, nix_on_node) {
            (
                j,
                "このホストの nix のビルドは計算ノードに投げられ（nix-daemon の中で走り taskset も効かない）、ジョブが終わるまで待つか、--builders '' --max-jobs auto を付けて手元でビルドしてください。",
            )
        } else if let Some(j) = nix_on_node.or(sent) {
            (
                j,
                "nix のビルドは nix-daemon の中で走り taskset が効かないので、ジョブが終わるまで待ってください。",
            )
        } else if let Some(j) = heavy_on_node {
            (
                j,
                "taskset で 1 コアに限る、終わるまで待つ、または slurm-run.sh で投げてください。",
            )
        } else {
            return Vec::new();
        };
        let (name, jobid) = (job.name.as_str(), job.id.as_str());
        vec![Finding::Deny(format!(
            "排他の Slurm のジョブ {name}（{jobid}）が実行中で、その時間の記録を乱します。{fix}"
        ))]
    }
}
