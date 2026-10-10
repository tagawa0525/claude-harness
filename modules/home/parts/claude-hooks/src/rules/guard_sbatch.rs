//! guard-sbatch: Slurm への投入を slurm-run.sh に寄せ、終わりを background で待たせる
//!
//! sbatch を直接投げると、名前・ログ・見積もり・連絡先が抜け、待ち行列から誰の何の計算かが
//! 読めなくなる（docs/adr/0007）。slurm-run.sh はジョブが終わるまで戻らず、待ち行列の長さ次第で
//! Bash ツールのフォアグラウンドの上限を超えるので、run_in_background=true でのみ許可する。
//! 回避する正当な理由がないため、エスケープは設けない。
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

/// コマンドとして実行されているか（直接、または `bash <path>` / `sh <path>` 経由）
fn runs(cmd: &Cmd, program: &str) -> bool {
    if basename(&cmd.name) == program {
        return true;
    }
    (cmd.name == "bash" || cmd.name == "sh")
        && cmd
            .args
            .first()
            .is_some_and(|first| basename(&first.text) == program)
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
