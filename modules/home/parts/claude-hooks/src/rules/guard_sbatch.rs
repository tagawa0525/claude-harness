//! guard-sbatch: Slurm への投入を slurm-run.sh に寄せ、終わりを background で待たせる
//!
//! sbatch を直接投げると、名前・ログ・見積もり・連絡先が抜け、待ち行列から誰の何の計算かが
//! 読めなくなる（docs/adr/0007）。slurm-run.sh はジョブが終わるまで戻らず、待ち行列の長さ次第で
//! Bash ツールのフォアグラウンドの上限を超えるので、run_in_background=true でのみ許可する。
//! `env`・`command` などで包んでも、shell.rs がほどいた中のプログラムで判定する。
//! slurm-run.sh が扱わない投げ方（ジョブの配列、依存、排他でない資源など）や、空いた待ち行列での
//! 短い計算のために、わかって外すときのエスケープを設ける: `ALLOW_RAW_SBATCH=1`（sbatch の直接の投入）、
//! `ALLOW_FOREGROUND_SLURM=1`（slurm-run.sh のフォアグラウンド実行）。
//! `echo sbatch` や `grep sbatch …` のように引数に現れるだけの場合は対象外。

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Cmd, Shell};
use crate::unwrap::basename;

pub struct GuardSbatch;

const SCRIPT: &str = "slurm-run.sh";
const TEST: &str = ".claude/tests/scripts/slurm-run.sh";

/// コマンドとして実行されているか（直接、前置き越し、または `bash <path>` / `sh <path>` 経由）
fn runs(cmd: &Cmd, target: &str) -> bool {
    basename(cmd.program()) == target
}

/// slurm-run.sh を実行しているか。同名のテスト（`.claude/tests/scripts/slurm-run.sh`）は偽物の sbatch で
/// 走り、Slurm に触れないので除く。本物の書き方は `~`、`$HOME`、相対、nix store など数え切れないので、
/// 除くのはテストの 1 つだけにして、見落とした書き方は止める側に倒す（docs/adr/0020）
fn runs_slurm_run(cmd: &Cmd) -> bool {
    let program = cmd.program();
    basename(program) == SCRIPT && !is_own_test(program)
}

fn is_own_test(path: &str) -> bool {
    path.strip_suffix(TEST)
        .is_some_and(|dir| dir.is_empty() || dir.ends_with('/'))
}

impl Rule for GuardSbatch {
    fn name(&self) -> &'static str {
        "guard-sbatch"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        let commands = shell.commands();
        if !shell.has_escape("ALLOW_RAW_SBATCH") && commands.iter().any(|cmd| runs(cmd, "sbatch")) {
            return vec![Finding::Deny(
                "sbatch を直接使わず、~/.claude/scripts/slurm-run.sh -t <見積もり> <名前> '<コマンド>' で投げてください（名前・ログ・見積もり・連絡先を付けます。ADR-0007）。slurm-run.sh で扱えない投げ方だとわかっていれば ALLOW_RAW_SBATCH=1 を付けます".to_string(),
            )];
        }
        if !input.run_in_background
            && !shell.has_escape("ALLOW_FOREGROUND_SLURM")
            && commands.iter().any(runs_slurm_run)
        {
            return vec![Finding::Deny(
                "slurm-run.sh はジョブが終わるまで待つので、Bash ツールの run_in_background=true で実行してください（関係するジョブはまとめて 1 つのコマンドで待ちます）。待ち行列が空で数分で終わるとわかっていれば ALLOW_FOREGROUND_SLURM=1 を付けます".to_string(),
            )];
        }
        Vec::new()
    }
}
