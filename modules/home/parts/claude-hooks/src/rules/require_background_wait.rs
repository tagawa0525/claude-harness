//! require-background-wait: 長時間待機するスクリプトのフォアグラウンド実行を止める
//!
//! gh-wait-review.sh / request-rereview.sh は漸増バックオフで約 10 分待つため、Bash ツールの
//! フォアグラウンド上限を必ず超えて失敗する。run_in_background=true でのみ許可する。
//! 回避する正当な理由がないため、エスケープは設けない。
//! ラッパー（`bash <path>`、sudo など）と `bash -c` の文字列の中は shell.rs がほどき、実行するプログラムを
//! 決められない形（`$WAIT 12`）はスクリプト名が文字列にあれば止める。
//! `echo request-rereview.sh` や `cat …` のように引数に現れるだけの場合は対象外。

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;
use crate::unwrap::basename;

pub struct RequireBackgroundWait;

const SCRIPTS: &[&str] = &["gh-wait-review.sh", "request-rereview.sh"];

impl Rule for RequireBackgroundWait {
    fn name(&self) -> &'static str {
        "require-background-wait"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        let runs_wait_script = SCRIPTS
            .iter()
            .any(|script| shell.runs_program(script, |program| basename(program) == *script));
        if input.run_in_background || !runs_wait_script {
            return Vec::new();
        }
        vec![Finding::Deny(
            "gh-wait-review.sh / request-rereview.sh は約10分待機するため、Bash ツールの run_in_background=true で実行してください（フォアグラウンドでは上限に達して必ず失敗します）".to_string(),
        )]
    }
}
