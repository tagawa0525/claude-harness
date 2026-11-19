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
//!
//! /slurm-run の「順番」と「複数のジョブを待つ」で使わない操作も止め、代わりの手を示す:
//! `scontrol hold|uhold|release|suspend|resume|requeuehold` と `update` の Priority・Nice・Partition・QOS（大文字と略記も。held のジョブは
//! ほかのエージェントが取り消しうる。main のジョブを止めて再開するのは
//! `--timed` のジョブだけ）、利用者・名前・状態などで絞る `scancel`（ほかのエージェントのジョブも取り消す）、
//! `slurm-run.sh` の出力のパイプと、同じスコープで後ろに続く表示のコマンド（`tail`、`head`、`grep`、`cat`。終了コードと
//! `TIME:` の行が消えるか、表示のコマンドの終了コードになる）。前の 2 つは `ALLOW_SLURM_CONTROL=<理由>` で外せる。
//! 表示のコマンドは slurm-run.sh と同じか外側のスコープのものだけを数え、`slurm-run.sh` の引数の `$(cat …)` は数えない。

use super::{Rule, apply_escape, escape_hint};
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Cmd, Shell};
use crate::unwrap::basename;

pub struct GuardSbatch;

pub(super) const SCRIPT: &str = "slurm-run.sh";
pub(super) const WAIT_SCRIPT: &str = "slurm-wait.sh";

/// slurm-run.sh か。同名のテスト（`claude/tests/scripts/slurm-run.sh`）は偽物の sbatch で走り、Slurm に
/// 触れないので除く。本物の書き方は `~`、`$HOME`、相対、nix store など数え切れないので、除くのはテストの
/// 1 つだけにして、見落とした書き方は止める側に倒す（docs/adr/0020）
pub(super) fn is_slurm_run(program: &str) -> bool {
    basename(program) == SCRIPT && !is_own_test(program, SCRIPT)
}

/// slurm-wait.sh か（同名のテストを除くのは [`is_slurm_run`] と同じ）
pub(super) fn is_slurm_wait(program: &str) -> bool {
    basename(program) == WAIT_SCRIPT && !is_own_test(program, WAIT_SCRIPT)
}

fn is_own_test(path: &str, script: &str) -> bool {
    path.strip_suffix(&format!("claude/tests/scripts/{script}"))
        .is_some_and(|dir| dir.is_empty() || dir.ends_with('/'))
}

/// 順番と止めることを変える scontrol のサブコマンド（`top` は ORDER: の行が示すので含めない）。resume は、timed のジョブが
/// 止めた main のジョブを測定の間に再開する
const SCONTROL_DENIED: &[&str] = &[
    "hold",
    "uhold",
    "release",
    "suspend",
    "resume",
    "requeuehold",
];
/// scontrol update の、待ち行列の順か止めることを変える欄
const UPDATE_DENIED: &[&str] = &["priority=", "nice=", "partition=", "qos="];
/// scontrol のオプションのうち、次の語を値に取るもの（その値をサブコマンドと読まない）
const SCONTROL_VALUED: &[&str] = &["-M", "--clusters"];

/// scancel の、番号でなく利用者・名前・状態などでジョブを選ぶオプション（長い形は `=値` でも書ける）
const SCANCEL_FILTERS_LONG: &[&str] = &[
    "--user",
    "--name",
    "--jobname",
    "--me",
    "--state",
    "--partition",
    "--account",
    "--nodelist",
    "--qos",
    "--reservation",
    "--wckey",
];
/// 短い形（`-u tagawa` も `-utagawa` も）
const SCANCEL_FILTERS_SHORT: &[char] = &['u', 'n', 't', 'p', 'A', 'w', 'q', 'R'];

/// slurm-run.sh の出力の後ろにつなぐと、終了コードと TIME: の行を隠す表示のコマンド
const DISPLAY: &[&str] = &["tail", "head", "grep", "cat"];

/// scontrol のサブコマンドの位置（値を取るオプションの値を飛ばす）
fn scontrol_subcommand(cmd: &Cmd) -> Option<usize> {
    let mut i = 0;
    while let Some(a) = cmd.args.get(i) {
        if !a.text.starts_with('-') {
            return Some(i);
        }
        i += if SCONTROL_VALUED.contains(&a.text.as_str()) {
            2
        } else {
            1
        };
    }
    None
}

/// scontrol はサブコマンドの大文字と小文字を区別せず、一意な略記（`ho`、`rel`）も受ける。requeue とその略記
/// （`requeu`）は requeuehold の頭でもあるが、scontrol は requeue と読むので数えない
fn scontrol_denied(cmd: &Cmd) -> bool {
    if basename(cmd.program()) != "scontrol" {
        return false;
    }
    let Some(i) = scontrol_subcommand(cmd) else {
        return false;
    };
    let sub = cmd.args[i].text.to_ascii_lowercase();
    let denied = !"requeue".starts_with(&sub)
        && SCONTROL_DENIED
            .iter()
            .any(|d| *d == sub || (sub.len() >= 2 && d.starts_with(&sub)));
    // 優先度を書き換えると、0 は hold と同じで、ほかの値は区分の順を崩す。nice と QOS は優先度を、パーティションは
    // timed に移すと main のジョブを止める
    let priority = (sub.len() >= 2 && "update".starts_with(&sub))
        && cmd.args[i + 1..].iter().any(|a| {
            let key = a.text.to_ascii_lowercase();
            UPDATE_DENIED.iter().any(|k| key.starts_with(k))
        });
    denied || priority
}

/// scancel の短いオプションのうち、値を取るもの（束ねた形の、これより後ろの文字は値）
const SCANCEL_VALUED_SHORT: &[char] = &['s', 'M'];

/// 長いオプションは一意な略記（`--us=`、`--stat`）も受けるので、名前が 2 文字以上でフィルタのどれかの頭なら数える。
/// 短いオプションは束ねられる（`-iu`）ので、値を取るオプションの文字まで 1 文字ずつ見る
fn scancel_filters(cmd: &Cmd) -> bool {
    basename(cmd.program()) == "scancel"
        && cmd.args.iter().any(|a| {
            let t = a.text.as_str();
            if let Some(long) = t.strip_prefix("--") {
                let name = long.split_once('=').map_or(long, |(n, _)| n);
                name.len() >= 2
                    && SCANCEL_FILTERS_LONG
                        .iter()
                        .any(|o| o.trim_start_matches('-').starts_with(name))
            } else if let Some(short) = t.strip_prefix('-') {
                for c in short.chars() {
                    if SCANCEL_FILTERS_SHORT.contains(&c) {
                        return true;
                    }
                    if SCANCEL_VALUED_SHORT.contains(&c) {
                        break;
                    }
                }
                false
            } else {
                false
            }
        })
}

/// slurm-run.sh の出力をパイプにつなぐか、後ろに表示のコマンドが続くか。後ろのコマンドは、slurm-run.sh と同じか外側の
/// スコープのものだけを数える（サブシェルや `bash -c` の中の slurm-run.sh も、外の `| tail` や `; cat` が隠す）。
/// 内側のスコープは slurm-run.sh の引数の `$(cat …)` なので数えない
fn hides_slurm_run_output(shell: &Shell) -> bool {
    let cmds = shell.commands();
    cmds.iter()
        .filter(|c| is_slurm_run(c.program()))
        .any(|run| {
            !run.piped_into.is_empty()
                || cmds.iter().any(|c| {
                    c.start > run.start
                        && run.scope.starts_with(&c.scope)
                        && DISPLAY.contains(&basename(c.program()))
                })
        })
}

impl Rule for GuardSbatch {
    fn name(&self) -> &'static str {
        "guard-sbatch"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        let mut control = Vec::new();
        if shell.commands().iter().any(scontrol_denied) {
            control.push(Finding::Deny(format!(
                "scontrol hold / uhold / release / suspend / resume / requeuehold と、update の Priority、Nice、Partition、QOS は使わないでください（held のジョブはほかのエージェントが取り消しうる。main のジョブを止めて再開するのは --timed のジョブだけ）。順番は slurm-run.sh が投げるたびに区分の順に並べ直します。変えるときは、急ぐ数分のジョブを --top で投げる、slurm-queue.sh の ORDER: の行の scontrol top を実行する、走っているジョブの終わりを待てない数分の計算を --timed で投げる、のどれかにしてください（/slurm-run の「順番」）。{}",
                escape_hint("ALLOW_SLURM_CONTROL")
            )));
        }
        if shell.commands().iter().any(scancel_filters) {
            control.push(Finding::Deny(format!(
                "scancel を利用者・名前・状態などで絞ると、ほかのエージェントのジョブも取り消します。自分が投げたジョブ（JOB: の行で控えた番号）を scancel <番号> で 1 本ずつ取り消してください。{}",
                escape_hint("ALLOW_SLURM_CONTROL")
            )));
        }
        // 拒否の理由はまとめて示す（1 つずつだと、直すたびに次の拒否を受ける）
        let mut findings = apply_escape(
            shell.escape("ALLOW_SLURM_CONTROL"),
            "ALLOW_SLURM_CONTROL",
            control,
        );
        if hides_slurm_run_output(shell) {
            findings.push(Finding::Deny(
                "slurm-run.sh の出力はパイプや、後ろに ; && || でつないだ表示のコマンド（tail、head、grep、cat）に渡さず、そのまま受けてください（終了コードと TIME: の行が消えるか、表示のコマンドの終了コードになります）。表示は、通知の後に別の呼び出しで読んでください（複数のファイルの末尾は tail -n 3 a.txt b.txt）".to_string(),
            ));
        }
        // 2 つの検査は別々のエスケープで外す
        if shell.runs_program("sbatch", |program| basename(program) == "sbatch") {
            let deny = Finding::Deny(format!(
                "sbatch を直接使わず、~/.claude/scripts/slurm-run.sh --class <区分> --ref '<参照>' --purpose '<目的>' -t <見積もり> <名前> '<コマンド>' で投げてください（名前・ログ・見積もり・区分と目的・連絡先を付けます。区分と目的は /slurm-run）。{}",
                escape_hint("ALLOW_RAW_SBATCH")
            ));
            let raw = apply_escape(
                shell.escape("ALLOW_RAW_SBATCH"),
                "ALLOW_RAW_SBATCH",
                vec![deny],
            );
            if !raw.is_empty() {
                findings.extend(raw);
                return findings;
            }
        }
        if !input.run_in_background && shell.runs_program(SCRIPT, is_slurm_run) {
            let deny = Finding::Deny(format!(
                "slurm-run.sh はジョブが終わるまで待つので、Bash ツールの run_in_background=true で実行してください（関係するジョブはまとめて 1 つのコマンドで待ちます）。{}",
                escape_hint("ALLOW_FOREGROUND_SLURM")
            ));
            findings.extend(apply_escape(
                shell.escape("ALLOW_FOREGROUND_SLURM"),
                "ALLOW_FOREGROUND_SLURM",
                vec![deny],
            ));
        }
        findings
    }
}
