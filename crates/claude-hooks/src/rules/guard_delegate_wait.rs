//! guard-delegate-wait: /delegate の担当（hook の入力に `agent_id` がある呼び出し）が、線（55 分）を超える待ちを
//! 始めることを止める（ADR-0078 の 2026-11-20 の変更）
//!
//! - `slurm-run.sh`: 見積もり（`-t`。1 つのコマンドで複数を投げれば合計）と待ち行列の開始の見込みから、終わりの見込みを
//!   計算し（script/delegate_wait.rs の `outlook`）、線を超えるか、見込みが分からなければ止める。止める文は、見込みの値と
//!   内訳、投げずに報告して終えること、投げるコマンドを報告の「続きの作業」に書くことを示す
//! - `slurm-wait.sh`: 担当は使わない（待つなら wait-step.sh で、線を超えたら終える。引き継いで待つのは親）
//!
//! 親の呼び出し（`agent_id` が無い）は通す。担当か親かをスクリプトの側で分けると、`a && b` でつないだ依存のある
//! ジョブが、親でも途中で切れる（ADR-0078）ので、hook で分ける。
//!
//! `-t` の値は、字面で決まる形（`-t 30`、`-t '1:30:00'`）だけを読む。変数や置き換え（`-t "$T"`）、実行するプログラムを
//! 決められない形（`$CMD`、`source`）、繰り返し（`for`、`while`。投げる本数が決まらない）は、見込みが分からないものとして止める（担当は字面で書き直せ、止める側に倒しても
//! 失うものが無い。ADR-0025、ADR-0042）。エスケープは無い（線を担当の判断で外させないため）。
//!
//! 待ち行列は squeue で読む。読めなければ見込みが分からないものとして止める。

use super::Rule;
use super::guard_sbatch::{SCRIPT, WAIT_SCRIPT, is_slurm_run, is_slurm_wait};
use crate::input::Input;
use crate::output::Finding;
use crate::script::delegate_wait::{self, LIMIT_SECS, Outlook};
use crate::script::slurm;
use crate::shell::{Cmd, Piece, Shell};

pub struct GuardDelegateWait;

/// slurm-run.sh の、値を取るオプション（slurm_run.rs の parse_args と同じ）
const VALUED: &[&str] = &["--class", "--ref", "--purpose", "-t", "-c"];
/// 値を取らないオプション
const FLAGS: &[&str] = &["--top", "--timed"];

/// 1 つの slurm-run.sh の呼び出しの見積もり（分）。slurm-run は `-t` を重ねると最後のものを使うので、最後の `-t` を
/// 読む。字面で決まらなければ理由
fn estimate_of(cmd: &Cmd) -> Result<u64, String> {
    let mut estimate = None;
    let mut i = 0;
    while let Some(arg) = cmd.args.get(i) {
        let text = arg.text.as_str();
        if FLAGS.contains(&text) {
            i += 1;
            continue;
        }
        if !VALUED.contains(&text) {
            break;
        }
        if text == "-t" {
            let Some(value) = cmd.args.get(i + 1) else {
                return Err("-t の値が無い".to_string());
            };
            if !value.pieces.iter().all(|p| matches!(p, Piece::Lit(_))) {
                return Err(format!(
                    "-t の値 {} が字面で決まらない（変数や置き換え）",
                    value.raw
                ));
            }
            estimate = Some(
                slurm::estimate_minutes(&value.text)
                    .ok_or_else(|| format!("-t の値 '{}' を見積もりとして読めない", value.text))?,
            );
        }
        i += 2;
    }
    estimate.ok_or_else(|| "-t が無い".to_string())
}

/// コマンドの slurm-run.sh の見積もりの合計（分）。どれかが字面で決まらなければ理由
fn total_estimate(shell: &Shell) -> Result<u64, String> {
    let runs: Vec<&Cmd> = shell
        .commands()
        .iter()
        .filter(|c| is_slurm_run(c.program()))
        .collect();
    if runs.is_empty() || shell.commands().iter().any(|c| c.opaque) {
        return Err("実行するプログラムを字面から決められない".to_string());
    }
    if shell.has_loop() {
        return Err("繰り返しの中で投げる本数が字面から決まらない".to_string());
    }
    runs.iter()
        .map(|cmd| estimate_of(cmd))
        .sum::<Result<u64, String>>()
}

/// 見込みを計算する（待ち行列を読む）
fn run_outlook(shell: &Shell) -> Outlook {
    let estimate = match total_estimate(shell) {
        Ok(minutes) => minutes,
        Err(reason) => return Outlook::Unknown(reason),
    };
    match slurm::queued_jobs() {
        Ok(queue) => delegate_wait::outlook(delegate_wait::now(), &queue, estimate),
        Err(slurm::QueueError::Spawn { error, .. }) => {
            Outlook::Unknown(format!("squeue を起動できない（{error}）"))
        }
        Err(slurm::QueueError::Failed(code)) => {
            Outlook::Unknown(format!("squeue が終了コード {code} で失敗した"))
        }
        Err(slurm::QueueError::Unreadable(reason)) => Outlook::Unknown(reason),
    }
}

/// 止める文
fn deny_run(outlook: &Outlook) -> String {
    format!(
        "担当は、終わりが {} 分を超える見込みか、見込みの分からない計算を投げません。{}。投げずに、/delegate の「作業中の規則」の 55 分を超える見込みの待ちの項のとおり、投げる前に済ませること（コミット、既定ブランチの進みの確認と rebase、チェックとレビューの取り込み）を済ませ、このコマンドを報告の「続きの作業」の「親が投げるコマンド」に書いて、報告して終えてください（投げて待つのは親です）",
        LIMIT_SECS / 60,
        delegate_wait::describe(outlook)
    )
}

const DENY_WAIT: &str = "slurm-wait.sh は、担当が終えた後に親がジョブを引き継いで待つコマンドです。担当は使いません。自分で投げたジョブは wait-step.sh で待ち（/delegate の「作業中の規則」の待ち方）、wait-step.sh が待ちの終わりを出したら、ジョブは止めずに、残りの作業を報告の「続きの作業」に書いて終えてください";

impl Rule for GuardDelegateWait {
    fn name(&self) -> &'static str {
        "guard-delegate-wait"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        if input.agent_id.is_empty() {
            return Vec::new();
        }
        let mut findings = Vec::new();
        if shell.runs_program(WAIT_SCRIPT, is_slurm_wait) {
            findings.push(Finding::Deny(DENY_WAIT.to_string()));
        }
        if shell.runs_program(SCRIPT, is_slurm_run) {
            let outlook = run_outlook(shell);
            if !outlook.within_limit() {
                findings.push(Finding::Deny(deny_run(&outlook)));
            }
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::total_estimate;
    use crate::shell::Shell;

    fn estimate(command: &str) -> Result<u64, String> {
        total_estimate(&Shell::parse(command))
    }

    #[test]
    fn the_estimate_is_read_from_the_literal_t_option() {
        let run = "~/.claude/scripts/slurm-run.sh --class trial --ref '#1' --purpose 'p'";
        assert_eq!(estimate(&format!("{run} -t 30 x true")), Ok(30));
        assert_eq!(
            estimate(&format!("{run} --top -t '1:30:00' x true")),
            Ok(90)
        );
        assert_eq!(
            estimate(&format!("cd /w && {run} -c 4 -t 2-00 x-shared true")),
            Ok(2880)
        );
    }

    #[test]
    fn estimates_of_jobs_in_one_command_are_added() {
        let run = "~/.claude/scripts/slurm-run.sh --class trial --ref '#1' --purpose 'p'";
        assert_eq!(
            estimate(&format!("{run} -t 30 a true && {run} -t 20 b true")),
            Ok(50)
        );
    }

    #[test]
    #[ignore = "RED: xargs、watch、関数の中の slurm-run.sh を 1 本と数える"]
    fn an_estimate_not_fixed_by_the_text_is_unknown() {
        let run = "~/.claude/scripts/slurm-run.sh --class trial --ref '#1' --purpose 'p'";
        for command in [
            format!("{run} -t \"$T\" x true"),
            format!("{run} -t $(cat t) x true"),
            format!("{run} x true"),
            format!("{run} -t 25m x true"),
            format!("{run} -t"),
            "$RUN -t 30 x true; echo slurm-run.sh".to_string(),
            format!("for n in 1 2 3; do {run} -t 30 x$n true; done"),
            format!("while true; do {run} -t 30 x true; done"),
            format!("until false; do {run} -t 30 x true; done"),
            format!("seq 3 | xargs -I{{}} {run} -t 30 x{{}} true"),
            format!("watch -n 60 {run} -t 5 x true"),
            format!("f() {{ {run} -t 30 x true; }}; f; f"),
            format!("bash -c 'for n in 1 2; do {run} -t 30 x true; done'"),
        ] {
            assert!(estimate(&command).is_err(), "{command}");
        }
    }

    #[test]
    fn a_repeated_t_is_read_as_the_last_one_like_slurm_run() {
        let command = "~/.claude/scripts/slurm-run.sh -t 5 --class trial --ref '#1' --purpose p -t 600 x true";
        assert_eq!(estimate(command), Ok(600));
    }

    #[test]
    fn the_purpose_that_looks_like_an_option_is_skipped_as_a_value() {
        let command =
            "~/.claude/scripts/slurm-run.sh --class trial --ref '#1' --purpose -t -t 5 x true";
        assert_eq!(estimate(command), Ok(5));
    }
}
