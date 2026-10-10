//! guard-sbatch: Slurm への投入を slurm-run.sh に寄せ、終わりを background で待たせる
//!
//! sbatch を直接投げると、名前・ログ・見積もり・連絡先が抜け、待ち行列から誰の何の計算かが
//! 読めなくなる（docs/adr/0007）。slurm-run.sh はジョブが終わるまで戻らず、待ち行列の長さ次第で
//! Bash ツールのフォアグラウンドの上限を超えるので、run_in_background=true でのみ許可する。
//! `command`・`env`・`exec`・`nohup`・`time` で包んでも、中のプログラムで判定する。
//! slurm-run.sh が扱わない投げ方（ジョブの配列、依存、排他でない資源など）や、空いた待ち行列での
//! 短い計算のために、わかって外すときのエスケープを設ける: `ALLOW_RAW_SBATCH=1`（sbatch の直接の投入）、
//! `ALLOW_FOREGROUND_SLURM=1`（slurm-run.sh のフォアグラウンド実行）。
//! `echo sbatch` や `grep sbatch …` のように引数に現れるだけの場合は対象外。

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Cmd, Shell};
use std::collections::VecDeque;

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

/// `env -S <文字列>`（`-S<文字列>`、`--split-string=<文字列>` も）の文字列。env はこれを空白で
/// 語に分けて、続く語の前に置く
fn split_string(option: &str, words: &mut VecDeque<String>) -> Option<String> {
    match option {
        "-S" | "--split-string" => words.pop_front(),
        _ => option
            .strip_prefix("--split-string=")
            .or_else(|| option.strip_prefix("-S"))
            .map(str::to_string),
    }
}

/// 前置きと `bash <path>` / `sh <path>` を外して、実際に走るプログラムの名前を返す
fn program(cmd: &Cmd) -> String {
    let mut words: VecDeque<String> = std::iter::once(cmd.name.clone())
        .chain(cmd.args.iter().map(|a| a.text.clone()))
        .collect();
    while let Some(word) = words.pop_front() {
        let name = basename(&word).to_string();
        if let Some((_, valued)) = WRAPPERS.iter().find(|(wrapper, _)| *wrapper == name) {
            while words
                .front()
                .is_some_and(|w| w.starts_with('-') || (name == "env" && w.contains('=')))
            {
                let option = words.pop_front().unwrap_or_default();
                if name == "env"
                    && let Some(split) = split_string(&option, &mut words)
                {
                    for part in split.split_whitespace().rev() {
                        words.push_front(part.to_string());
                    }
                } else if valued.contains(&option.as_str()) {
                    words.pop_front();
                }
            }
            continue;
        }
        if name == "bash" || name == "sh" {
            return words.pop_front().map_or(name, |w| basename(&w).to_string());
        }
        return name;
    }
    String::new()
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
        if !shell.has_escape("ALLOW_RAW_SBATCH") && commands.iter().any(|cmd| runs(cmd, "sbatch")) {
            return vec![Finding::Deny(
                "sbatch を直接使わず、~/.claude/scripts/slurm-run.sh -t <見積もり> <名前> '<コマンド>' で投げてください（名前・ログ・見積もり・連絡先を付けます。ADR-0007）。slurm-run.sh で扱えない投げ方だとわかっていれば ALLOW_RAW_SBATCH=1 を付けます".to_string(),
            )];
        }
        if !input.run_in_background
            && !shell.has_escape("ALLOW_FOREGROUND_SLURM")
            && commands.iter().any(|cmd| runs(cmd, SCRIPT))
        {
            return vec![Finding::Deny(
                "slurm-run.sh はジョブが終わるまで待つので、Bash ツールの run_in_background=true で実行してください（関係するジョブはまとめて 1 つのコマンドで待ちます）。待ち行列が空で数分で終わるとわかっていれば ALLOW_FOREGROUND_SLURM=1 を付けます".to_string(),
            )];
        }
        Vec::new()
    }
}
