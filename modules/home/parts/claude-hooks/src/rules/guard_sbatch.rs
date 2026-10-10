//! guard-sbatch: Slurm への投入を slurm-run.sh に寄せ、終わりを background で待たせる
//!
//! sbatch を直接投げると、名前・ログ・見積もり・連絡先が抜け、待ち行列から誰の何の計算かが
//! 読めなくなる（docs/adr/0007）。slurm-run.sh はジョブが終わるまで戻らず、待ち行列の長さ次第で
//! Bash ツールのフォアグラウンドの上限を超えるので、run_in_background=true でのみ許可する。
//! 回避する正当な理由がないため、エスケープは設けない。`command`・`env`・`exec`・`nohup`・`time` で
//! 包んでも、中のプログラムで判定する。
//! `echo sbatch` や `grep sbatch …` のように引数に現れるだけの場合は対象外。

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Cmd, Shell};

pub struct GuardSbatch;

const SCRIPT: &str = "slurm-run.sh";

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// 引数のプログラムをそのまま走らせる前置きと、そのうち値を別の語で取るオプション
/// （env と GNU time は coreutils と time の man、exec は bash の組み込み）。後ろのオプション
/// （`-` で始まる語。`--unset=FOO` のように値をつないだ形を含む）と、`env` の変数の代入
/// （`NAME=value`）を読み飛ばす
const WRAPPERS: &[(&str, &[&str])] = &[
    ("command", &[]),
    ("env", &["-u", "--unset", "-C", "--chdir"]),
    ("exec", &["-a"]),
    ("nohup", &[]),
    ("time", &["-f", "--format", "-o", "--output"]),
];

/// 前置きと `bash <path>` / `sh <path>` を外して、実際に走るプログラムの名前を返す
fn program(cmd: &Cmd) -> &str {
    let words = std::iter::once(cmd.name.as_str()).chain(cmd.args.iter().map(|a| a.text.as_str()));
    let mut words = words.peekable();
    while let Some(word) = words.next() {
        let name = basename(word);
        if let Some((_, valued)) = WRAPPERS.iter().find(|(wrapper, _)| *wrapper == name) {
            while let Some(option) =
                words.next_if(|w| w.starts_with('-') || (name == "env" && w.contains('=')))
            {
                if valued.contains(&option) {
                    words.next();
                }
            }
            continue;
        }
        if name == "bash" || name == "sh" {
            return words.next().map_or(name, basename);
        }
        return name;
    }
    ""
}

/// コマンドとして実行されているか（直接、前置き越し、または `bash <path>` / `sh <path>` 経由）
fn runs(cmd: &Cmd, target: &str) -> bool {
    program(cmd) == target
}

impl Rule for GuardSbatch {
    fn name(&self) -> &'static str {
        "guard-sbatch"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        let commands = shell.commands();
        if commands.iter().any(|cmd| runs(cmd, "sbatch")) {
            return vec![Finding::Deny(
                "sbatch を直接使わず、~/.claude/scripts/slurm-run.sh -t <見積もり> <名前> '<コマンド>' で投げてください（名前・ログ・見積もり・連絡先を付けます。ADR-0007）".to_string(),
            )];
        }
        if !input.run_in_background && commands.iter().any(|cmd| runs(cmd, SCRIPT)) {
            return vec![Finding::Deny(
                "slurm-run.sh はジョブが終わるまで待つので、Bash ツールの run_in_background=true で実行してください（関係するジョブはまとめて 1 つのコマンドで待ちます）".to_string(),
            )];
        }
        Vec::new()
    }
}
