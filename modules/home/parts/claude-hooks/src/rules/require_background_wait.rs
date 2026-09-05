//! require-background-wait: 長時間待機するスクリプトのフォアグラウンド実行を止める
//!
//! gh-wait-review.sh / request-rereview.sh は漸増バックオフで約 10 分待つため、Bash ツールの
//! フォアグラウンド上限を必ず超えて失敗する。run_in_background=true でのみ許可する。
//! 回避する正当な理由がないため、エスケープは設けない。
//! `echo request-rereview.sh` や `cat …` のように引数に現れるだけの場合は対象外。

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Cmd, Shell};

pub struct RequireBackgroundWait;

const SCRIPTS: &[&str] = &["gh-wait-review.sh", "request-rereview.sh"];

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// コマンドとして実行されているか（直接、または `bash <path>` / `sh <path>` 経由）
fn runs_wait_script(cmd: &Cmd) -> bool {
    if SCRIPTS.contains(&basename(&cmd.name)) {
        return true;
    }
    if cmd.name == "bash" || cmd.name == "sh" {
        if let Some(first) = cmd.args.first() {
            return SCRIPTS.contains(&basename(&first.text));
        }
    }
    false
}

impl Rule for RequireBackgroundWait {
    fn name(&self) -> &'static str {
        "require-background-wait"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        if input.run_in_background || !shell.commands().iter().any(runs_wait_script) {
            return Vec::new();
        }
        vec![Finding::Deny(
            "gh-wait-review.sh / request-rereview.sh は約10分待機するため、Bash ツールの run_in_background=true で実行してください（フォアグラウンドでは上限に達して必ず失敗します）".to_string(),
        )]
    }
}
