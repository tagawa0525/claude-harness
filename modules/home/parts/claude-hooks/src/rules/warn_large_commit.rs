//! warn-large-commit: 大規模なコミットの件数をモデルに伝える（ブロックしない）
//!
//! 5 ファイル以上または 100 行以上なら additionalContext で件数と確認事項を渡す。
//! 数えるのは決定的なので hook が行い、「1 つの論理的変更に収まっているか」「分割するか」の
//! 判断はモデルに残す。

use super::Rule;
use crate::git;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Shell, short_cluster_has};

pub struct WarnLargeCommit;

/// `git diff --numstat` の出力を（ファイル数, 追加+削除行数）に集計する。バイナリ（`-`）は 0 扱い
fn count_numstat(numstat: &str) -> (usize, usize) {
    let mut files = 0;
    let mut lines = 0;
    for line in numstat.lines().filter(|l| !l.is_empty()) {
        files += 1;
        let mut it = line.split('\t');
        for _ in 0..2 {
            lines += it.next().and_then(|n| n.parse::<usize>().ok()).unwrap_or(0);
        }
    }
    (files, lines)
}

impl Rule for WarnLargeCommit {
    fn name(&self) -> &'static str {
        "warn-large-commit"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        for cmd in shell.commands() {
            let Some(("commit", args)) = cmd.git_subcommand() else {
                continue;
            };
            let dir = shell.target_dir(&cmd, &input.cwd);
            // -a / --all ならワーキングツリーの変更も含めて数える。
            // --amend / --author は `--` で始まるので短縮形のクラスタには当たらない
            let all = args
                .iter()
                .any(|a| a.text == "--all" || short_cluster_has(a, 'a'));
            let diff_args: &[&str] = if all {
                &["diff", "HEAD", "--numstat"]
            } else {
                &["diff", "--cached", "--numstat"]
            };
            let Some(numstat) = git::git(&dir, diff_args) else {
                continue;
            };
            let (files, lines) = count_numstat(&numstat);
            if files == 0 || (files < 5 && lines < 100) {
                continue;
            }
            return vec![Finding::Context(format!(
                "⚠️ 大規模な変更です（{files} ファイル、{lines} 行）。1 つの論理的変更に収まっているか確認してください。複数の変更が混在していれば git add でファイル単位（または git add -p で部分単位）に分け、別々にコミットしてください。大きくても 1 つの変更ならそのまま続行してよいです。"
            ))];
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::count_numstat;

    #[test]
    fn counts_files_and_lines_with_binary_as_zero() {
        assert_eq!(
            count_numstat("10\t2\ta.txt\n-\t-\tb.png\n3\t0\tc.txt\n"),
            (3, 15)
        );
        assert_eq!(count_numstat(""), (0, 0));
    }
}
