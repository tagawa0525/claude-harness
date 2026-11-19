//! restart-wait-clock: /delegate の担当（hook の入力に `agent_id` がある呼び出し）が親に送ったら、その担当の wait-step の
//! 時計を数え直す（ゲートではない。止めない。ADR-0078 の 2026-11-20 の変更）
//!
//! wait-step の線（55 分）は、親のキャッシュ（1 時間）が切れる前に担当が終えるためのもので、親に最後に送った時刻から測る。
//! 数え直しを担当の判断に任せないよう、送る呼び出しをここで見る。
//!
//! - Bash で `wait-step.sh <鍵> …` を呼んだら（`--end` を除く）、その担当がその鍵で待っていることを残す（鍵は字面で決まる
//!   ものだけ。決まらなければ残さず、数え直さないので線が早めに来る）
//! - SendMessage の `to` が `main` か、SubagentHandback なら、残した鍵の時計を今から数え直す（待っていなければ何もしない）
//!
//! PreToolUse で見るので、ほかのルールが止めた送信でも数え直す（止められた担当は、直してすぐ送り直す）。

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::script::wait_step;
use crate::shell::{Piece, Shell};
use crate::unwrap::basename;

pub struct RestartWaitClock;

const SCRIPT: &str = "wait-step.sh";

/// コマンドの wait-step.sh の、字面で決まる鍵（`--end` の呼び出しを除く）
fn waiting_keys(shell: &Shell) -> Vec<String> {
    shell
        .commands()
        .iter()
        .filter(|c| basename(c.program()) == SCRIPT)
        .filter_map(|c| c.args.first())
        .filter(|a| a.text != "--end" && a.pieces.iter().all(|p| matches!(p, Piece::Lit(_))))
        .map(|a| a.text.clone())
        .collect()
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Rule for RestartWaitClock {
    fn name(&self) -> &'static str {
        "restart-wait-clock"
    }

    fn tools(&self) -> &'static [&'static str] {
        &["Bash", "SendMessage", "SubagentHandback"]
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        if input.agent_id.is_empty() {
            return Vec::new();
        }
        match input.tool_name.as_str() {
            "Bash" => {
                for key in waiting_keys(shell) {
                    wait_step::note_waiter(&input.agent_id, &key);
                }
            }
            "SendMessage" if input.to == "main" => {
                wait_step::restart_from_contact(&input.agent_id, now())
            }
            "SubagentHandback" => wait_step::restart_from_contact(&input.agent_id, now()),
            _ => {}
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::waiting_keys;
    use crate::shell::Shell;

    #[test]
    fn the_literal_key_of_a_wait_is_kept_and_end_or_an_expanded_key_is_not() {
        let keys = |c: &str| waiting_keys(&Shell::parse(c));
        assert_eq!(
            keys("~/.claude/scripts/wait-step.sh /home/u/wt /tmp/a.output"),
            ["/home/u/wt"]
        );
        assert!(keys("~/.claude/scripts/wait-step.sh --end /home/u/wt").is_empty());
        assert!(keys("~/.claude/scripts/wait-step.sh \"$WT\"").is_empty());
        assert!(keys("echo wait-step.sh").is_empty());
    }
}
