//! pre-git-merge-check: 既定ブランチへのローカルの git merge の前に、ブランチのトピックが 1 つかを確かめる
//!
//! GitHub を使わないリポジトリは feature branch を main に `git merge --no-ff` する。
//! gh pr merge の pre-merge-check と同じく、ブランチが新しい ADR（docs/adr/ 直下の
//! NNNN-*.md）を 2 件以上加えていれば deny する。ADR は 1 件 1 決定なので、新しい ADR の数が
//! トピックの数の目安になる。既存の ADR の変更（status の superseded への変更など）は数えない。
//!
//! 対象はローカルのブランチ（refs/heads/）を既定ブランチの上でマージするときだけ。
//! リモート追跡ブランチの取り込み（`git merge origin/main`）は別々にマージ済みのトピックの
//! 集まりなので対象外。docs/adr の無いリポジトリでは数える ADR が無いので何もしない。
//! エスケープ: `ALLOW_MULTI_TOPIC=1`（1 つの決定を複数の ADR に分けて書いたときだけ）

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Arg, Shell};

pub struct PreGitMergeCheck;

/// ADR のファイルか（docs/adr/ 直下の、数字で始まる .md）。README やテンプレートは含めない。
/// .claude/scripts/branch-topics.sh も同じ条件で数える
pub(super) fn is_adr(_path: &str) -> bool {
    todo!()
}

/// 新しい ADR が 2 件以上なら deny の理由
pub(super) fn multi_topic_reason(_new_adrs: &[String]) -> Option<String> {
    todo!()
}

/// `git merge args…` のマージ対象。中断・再開（--abort / --continue / --quit）なら None
fn merge_targets(_args: &[Arg]) -> Option<Vec<String>> {
    todo!()
}

impl Rule for PreGitMergeCheck {
    fn name(&self) -> &'static str {
        "pre-git-merge-check"
    }

    fn check(&self, _input: &Input, shell: &Shell) -> Vec<Finding> {
        for cmd in shell.commands() {
            if let Some(("merge", args)) = cmd.git_subcommand() {
                let _ = merge_targets(args);
            }
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(src: &str) -> Option<Vec<String>> {
        let cs = Shell::parse(src).commands();
        let (_, args) = cs[0].git_subcommand().unwrap();
        merge_targets(args)
    }

    #[test]
    fn adr_files() {
        assert!(is_adr("docs/adr/0002-use-b.md"));
        assert!(!is_adr("docs/adr/README.md"));
        assert!(!is_adr("docs/adr/template.md"));
        assert!(!is_adr("docs/adr/drafts/0003-x.md"));
        assert!(!is_adr("docs/adrs/0004-x.md"));
        assert!(!is_adr("sub/docs/adr/0005-x.md"));
        assert!(!is_adr("docs/adr/0006-x.txt"));
    }

    #[test]
    fn reason_only_for_two_or_more() {
        assert_eq!(multi_topic_reason(&[]), None);
        assert_eq!(multi_topic_reason(&["docs/adr/0002-a.md".into()]), None);
        let r = multi_topic_reason(&["docs/adr/0002-a.md".into(), "docs/adr/0003-b.md".into()])
            .unwrap();
        assert!(r.contains("docs/adr/0002-a.md") && r.contains("docs/adr/0003-b.md"));
        assert!(r.contains("/topic-triage"));
        assert!(r.contains("ALLOW_MULTI_TOPIC=1"));
    }

    #[test]
    fn merge_targets_skip_option_values() {
        assert_eq!(
            targets("git merge --no-ff feat/a -m 'Merge: a'"),
            Some(vec!["feat/a".to_string()])
        );
        assert_eq!(
            targets("git merge -m x -s ort -X theirs --into-name main feat/a feat/b"),
            Some(vec!["feat/a".to_string(), "feat/b".to_string()])
        );
        assert_eq!(
            targets("git merge -F msg.txt --log=3 feat/a"),
            Some(vec!["feat/a".to_string()])
        );
        assert_eq!(targets("git merge -"), Some(vec!["-".to_string()]));
    }

    #[test]
    fn merge_control_is_not_a_target() {
        assert_eq!(targets("git merge --abort"), None);
        assert_eq!(targets("git merge --continue"), None);
        assert_eq!(targets("git merge --quit"), None);
    }
}
