//! guard-gh-run-rerun: 既に再実行済みの run に対する gh run rerun を止める
//!
//! 一時障害（GitHub API の 5xx、Copilot の内部エラー）の再実行は 1 回まで。attempt 2 以上で
//! 同じ失敗が続くなら一時障害ではないので、無制限に再実行せず原因を報告してユーザーの判断に委ねる。
//! attempt は gh run view <id> --json attempt で確認する。確認できなければ deny する
//! （確認できない状態で再実行させない）。エスケープ: コマンドに `ALLOW_RERUN=1` を付ける

use super::Rule;
use crate::gh;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Arg, Shell};

pub struct GuardGhRunRerun;

fn deny(reason: &str) -> Finding {
    Finding::Deny(format!(
        "{reason}\n\nどうしても必要な場合は ALLOW_RERUN=1 をコマンドに付けて実行してください。"
    ))
}

/// 引数から (-R の値, run ID) を取り出す。--job の値（数値のジョブ ID もある）は run ID ではない
fn parse(args: &[Arg]) -> (Option<String>, Option<String>) {
    let mut repo = None;
    let mut run_id = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let t = a.text.as_str();
        match t {
            "-R" | "--repo" => repo = it.next().map(|v| v.text.clone()),
            "-j" | "--job" => {
                it.next();
            }
            _ if t.starts_with("--repo=") => repo = Some(t["--repo=".len()..].to_string()),
            _ if t.starts_with('-') => {}
            _ => {
                if run_id.is_none() && t.chars().all(|c| c.is_ascii_digit()) {
                    run_id = Some(t.to_string());
                }
            }
        }
    }
    (repo, run_id)
}

impl Rule for GuardGhRunRerun {
    fn name(&self) -> &'static str {
        "guard-gh-run-rerun"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        if shell.has_escape("ALLOW_RERUN") {
            return Vec::new();
        }
        for cmd in shell.commands() {
            let Some(args) = cmd.gh_subcommand(&["run", "rerun"]) else {
                continue;
            };
            let (repo, Some(run_id)) = parse(args) else {
                continue;
            };
            let dir = shell.target_dir(&cmd, &input.cwd);
            let mut argv: Vec<&str> = vec!["run", "view"];
            if let Some(r) = repo.as_deref() {
                argv.extend(["-R", r]);
            }
            argv.extend([run_id.as_str(), "--json", "attempt"]);
            let attempt = gh::gh(&dir, &argv)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .and_then(|v| v["attempt"].as_u64());
            let Some(attempt) = attempt else {
                return vec![deny(&format!(
                    "run {run_id} の試行回数を確認できません（gh run view が失敗）。再実行の前に /gh-actions-check で状況を診断してください"
                ))];
            };
            if attempt >= 2 {
                return vec![deny(&format!(
                    "run {run_id} は既に {attempt} 回試行して失敗しています。一時障害の再実行は 1 回までです。同じ失敗が続くなら一時障害ではないので、~/.claude/scripts/gh-actions-diagnose.sh の結果（CAUSE / errors）を添えてユーザーに報告し、判断を仰いでください"
                ))];
            }
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::parse;
    use crate::shell::Shell;

    fn p(src: &str) -> (Option<String>, Option<String>) {
        let cs = Shell::parse(src).commands();
        parse(cs[0].gh_subcommand(&["run", "rerun"]).unwrap())
    }

    #[test]
    fn job_value_is_not_run_id() {
        assert_eq!(p("gh run rerun --job 555 100"), (None, Some("100".into())));
        assert_eq!(p("gh run rerun 100 -j 555"), (None, Some("100".into())));
        assert_eq!(
            p("gh run rerun -R octo/repo 100 --failed"),
            (Some("octo/repo".into()), Some("100".into()))
        );
    }
}
