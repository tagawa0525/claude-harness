//! guard-sbatch: Slurm への投入を slurm-run.sh に寄せ、終わりを background で待たせる
//!
//! sbatch を直接投げると、名前・ログ・見積もり・連絡先が抜け、待ち行列から誰の何の計算かが
//! 読めなくなる（docs/adr/0007）。slurm-run.sh はジョブが終わるまで戻らず、待ち行列の長さ次第で
//! Bash ツールのフォアグラウンドの上限を超えるので、run_in_background=true でのみ許可する。
//! `env`・`command`・`sudo` などで包んでも、`bash -x <path>` や `bash -c '…'` の形でも、shell.rs がほどいた中の
//! プログラムで判定する。実行するプログラムを決められない形（`$CMD`、`source`）は、対象語が文字列にあれば
//! 止める側に倒す（ADR-0025）。
//! slurm-run.sh が扱わない投げ方（ジョブの配列、依存など。排他にしない投げ方は `-c`、ADR-0029）や、空いた待ち行列での
//! 短い計算のために、わかって外すときのエスケープを設ける: `ALLOW_RAW_SBATCH=<理由>`（sbatch の直接の投入）、
//! `ALLOW_FOREGROUND_SLURM=<理由>`（slurm-run.sh のフォアグラウンド実行）。
//! `echo sbatch` や `grep sbatch …` のように引数に現れるだけの場合は対象外。

use super::{Rule, apply_escape, escape_hint};
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;
use crate::unwrap::basename;

pub struct GuardSbatch;

const SCRIPT: &str = "slurm-run.sh";
const TEST: &str = ".claude/tests/scripts/slurm-run.sh";

/// slurm-run.sh か。同名のテスト（`.claude/tests/scripts/slurm-run.sh`）は偽物の sbatch で走り、Slurm に
/// 触れないので除く。本物の書き方は `~`、`$HOME`、相対、nix store など数え切れないので、除くのはテストの
/// 1 つだけにして、見落とした書き方は止める側に倒す（docs/adr/0020）
fn is_slurm_run(program: &str) -> bool {
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
        // 2 つの検査は別々のエスケープで外す
        if shell.runs_program("sbatch", |program| basename(program) == "sbatch") {
            let deny = Finding::Deny(format!(
                "sbatch を直接使わず、~/.claude/scripts/slurm-run.sh -t <見積もり> <名前> '<コマンド>' で投げてください（名前・ログ・見積もり・連絡先を付けます）。{}",
                escape_hint("ALLOW_RAW_SBATCH")
            ));
            let findings = apply_escape(
                shell.escape("ALLOW_RAW_SBATCH"),
                "ALLOW_RAW_SBATCH",
                vec![deny],
            );
            if !findings.is_empty() {
                return findings;
            }
        }
        if !input.run_in_background && shell.runs_program(SCRIPT, is_slurm_run) {
            let deny = Finding::Deny(format!(
                "slurm-run.sh はジョブが終わるまで待つので、Bash ツールの run_in_background=true で実行してください（関係するジョブはまとめて 1 つのコマンドで待ちます）。{}",
                escape_hint("ALLOW_FOREGROUND_SLURM")
            ));
            return apply_escape(
                shell.escape("ALLOW_FOREGROUND_SLURM"),
                "ALLOW_FOREGROUND_SLURM",
                vec![deny],
            );
        }
        Vec::new()
    }
}
