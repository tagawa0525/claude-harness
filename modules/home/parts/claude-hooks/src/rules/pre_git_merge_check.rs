//! pre-git-merge-check: 既定ブランチへのローカルの git merge の前に、ブランチのトピックが 1 つかを確かめる
//!
//! GitHub を使わないリポジトリは feature branch を main に `git merge --no-ff` する。
//! gh pr merge の pre-merge-check と同じく、ブランチが新しい ADR（docs/adr/ 直下の
//! NNNN-*.md）を 2 件以上加えていれば deny する。ADR は 1 件 1 決定なので、新しい ADR の数が
//! トピックの数の目安になる。既存の ADR の変更（status の superseded への変更など）は数えない。
//!
//! 対象はローカルのブランチ（refs/heads/）を既定ブランチの上でマージするときだけ。今のブランチは
//! 同じコマンドの前の git switch / checkout を反映し（branch_at）、移り先が分からなければ確かめる。
//! リモート追跡ブランチの取り込み（`git merge origin/main`）は別々にマージ済みのトピックの
//! 集まりなので対象外。docs/adr の無いリポジトリでは数える ADR が無いので何もしない。
//! エスケープ: `ALLOW_MULTI_TOPIC=1`（1 つの決定を複数の ADR に分けて書いたときだけ）

use super::Rule;
use super::guard_branch_base::branch_at;
use crate::git;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Arg, Shell};

pub struct PreGitMergeCheck;

/// ADR のファイルか（docs/adr/ 直下の NNNN-*.md。4 桁の番号とハイフンで始まる）。
/// README やテンプレートは含めない。.claude/scripts/branch-topics.sh も同じ条件で数える
pub(super) fn is_adr(path: &str) -> bool {
    path.strip_prefix("docs/adr/").is_some_and(|name| {
        let b = name.as_bytes();
        !name.contains('/')
            && name.ends_with(".md")
            && b.len() > 5
            && b[..4].iter().all(u8::is_ascii_digit)
            && b[4] == b'-'
    })
}

/// 新しい ADR が 2 件以上なら deny の理由
pub(super) fn multi_topic_reason(new_adrs: &[String]) -> Option<String> {
    if new_adrs.len() < 2 {
        return None;
    }
    let list: Vec<String> = new_adrs.iter().map(|a| format!("  - {a}")).collect();
    Some(format!(
        "このブランチは新しい ADR を {} 件加えています（1 ブランチ 1 トピック、ADR は 1 件 1 決定）:\n{}\n\
         決定ごとにブランチを分けてください（/topic-triage）。1 つの決定を複数の ADR に分けて書いた場合に限り、\
         ALLOW_MULTI_TOPIC=1 を付けてマージし、マージのメッセージにその理由を書いてください。",
        new_adrs.len(),
        list.join("\n")
    ))
}

/// `git merge args…` のマージ対象。中断・再開（--abort / --continue / --quit）なら None。
/// 対象を標準入力で受け取る `--stdin` はそのまま対象に入れる（check で確かめられないとして止める）
fn merge_targets(args: &[Arg]) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let t = a.text.as_str();
        match t {
            "--abort" | "--continue" | "--quit" => return None,
            "-m" | "-F" | "--file" | "-s" | "--strategy" | "-X" | "--strategy-option"
            | "--into-name" => {
                it.next();
            }
            "-" | "--stdin" => out.push(t.to_string()),
            _ if t.starts_with('-') => {}
            _ => out.push(t.to_string()),
        }
    }
    Some(out)
}

impl Rule for PreGitMergeCheck {
    fn name(&self) -> &'static str {
        "pre-git-merge-check"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        if shell.has_escape("ALLOW_MULTI_TOPIC") {
            return Vec::new();
        }
        for cmd in shell.commands() {
            let Some(targets) = cmd
                .git_subcommand()
                .filter(|(sub, _)| *sub == "merge")
                .and_then(|(_, args)| merge_targets(args))
            else {
                continue;
            };
            let dir = shell.target_dir(&cmd, &input.cwd);
            let Some(default_ref) = git::default_branch(&dir) else {
                continue;
            };
            // 同じコマンドの前の git switch で移った先で判定する。分からなければ確かめる側に倒す
            if branch_at(shell, &cmd, &input.cwd, &dir)
                .is_some_and(|b| b != git::local_name(&default_ref))
            {
                continue;
            }
            for t in targets {
                if t == "--stdin" {
                    return vec![Finding::Deny(
                        "git merge --stdin はマージ対象を標準入力で受け取るので、新しい ADR の数を確かめられません。ブランチ名を引数で渡してください".to_string(),
                    )];
                }
                let spec = if t == "-" { "@{-1}" } else { t.as_str() };
                // ローカルのブランチだけ。解決できない名前は git merge 自身が失敗する
                let Some(full) = git::git(&dir, &["rev-parse", "--symbolic-full-name", spec])
                    .filter(|f| f.starts_with("refs/heads/"))
                else {
                    continue;
                };
                let Some(added) = git::git(
                    &dir,
                    &[
                        "diff",
                        // 改名は新しい ADR ではない。検出を diff.renames の設定に左右させない
                        "--find-renames",
                        "--diff-filter=A",
                        "--name-only",
                        // パスをクォートせずに NUL 区切りで受け取る（非 ASCII のファイル名）
                        "-z",
                        &format!("HEAD...{full}"),
                        "--",
                        "docs/adr",
                    ],
                ) else {
                    return vec![Finding::Deny(format!(
                        "{t} が加える ADR を確認できません（git diff HEAD...{full} が失敗）"
                    ))];
                };
                let adrs: Vec<String> = added
                    .split('\0')
                    .filter(|p| is_adr(p))
                    .map(str::to_string)
                    .collect();
                if let Some(reason) = multi_topic_reason(&adrs) {
                    return vec![Finding::Deny(reason)];
                }
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
        assert!(!is_adr("docs/adr/1-notes.md"));
        assert!(!is_adr("docs/adr/2026notes.md"));
        assert!(!is_adr("docs/adr/00012-x.md"));
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
    fn stdin_targets_are_unknown() {
        assert_eq!(
            targets("git merge --stdin"),
            Some(vec!["--stdin".to_string()])
        );
    }

    #[test]
    fn merge_control_is_not_a_target() {
        assert_eq!(targets("git merge --abort"), None);
        assert_eq!(targets("git merge --continue"), None);
        assert_eq!(targets("git merge --quit"), None);
    }
}
