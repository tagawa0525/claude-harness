//! `git-merge-state.sh [branch]`: /git-merge の事前の確認（対象の検証と、マージの前に要る状態の収集）。branch を省くと
//! 今のブランチを対象にする。次のときは ERROR を出して 1 で終える（マージに進まない）:
//! - 対象がローカルに無い、または既定のブランチ（main / master）自身
//! - GitHub のリモートがある（/gh-pr-create と /gh-pr-merge の流れで、ここでは扱わない）
//! - 対象か既定のブランチの worktree に未コミットの変更がある
//! - git の問い合わせが失敗した（ERROR の行を出す。欠けた行を「無い」と読まれないように）
//!
//! 既定のブランチはローカルの main / master（リモートの無いリポジトリ向けなので origin は見ない）。
//!
//! 出力（既定のブランチが無ければ COMMITS と BEHIND の代わりに ROOT_COMMITS）:
//! - `TARGET: <branch>`、`DEFAULT: <branch>|none`
//! - `COMMITS: <n>`: 既定のブランチにまだ入っていないコミットの数
//! - `BEHIND: <n>`: 対象にまだ入っていない既定のブランチのコミットの数（0 でなければ rebase）
//! - `ROOT_COMMITS: <hash> ...`: DEFAULT が none のとき、対象のルートのコミット
//! - `STACKED_ON: <branch>,...|none`: 対象の下に積まれている、まだマージしていない別のブランチ
//! - `TARGET_WORKTREE: <path>|none`、`DEFAULT_WORKTREE: <path>|none`
//! - `PROPOSED_ADRS: ADR-NNNN ...|none`: 対象のコミット（既定のブランチが無ければ全部）のうち、件名の型が feat か fix の
//!   ものが本文の `Refs:` の行で挙げた ADR（カンマで区切った値のうち ADR-NNNN そのもの）で、対象の先端で
//!   docs/adr/NNNN-*.md の status が proposed のもの（番号の順）。対象の先端に docs/adr が無ければ行を出さない
//!
//! 実装をマージするブランチは、実装した proposed の ADR を同じブランチで accepted にする。確定した ADR の凍結する部分は
//! マージの後には「## 変更」でしか直せないので、マージの前に見落としを一覧にする（止めはしない。一部だけを実装した
//! ブランチや方法を試すブランチは proposed のまま残してよく、機械では決められない）

use crate::git::git;
use regex::Regex;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;

static FEAT_FIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(feat|fix)(\([^)]*\))?!?: ").unwrap());
static ADR_REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^ADR-[0-9]{4}$").unwrap());

/// git の問い合わせが失敗したときの ERROR の行（失敗した問い合わせを添える）
fn failed(what: &str) -> i32 {
    for notice in crate::output::take_notices() {
        eprintln!("{notice}");
    }
    eprintln!(
        "ERROR: git-merge-state が {what} で失敗しました。上にメッセージがあればその原因を直してから、もう一度実行してください"
    );
    1
}

pub fn main(args: &[String]) -> i32 {
    let dir = Path::new(".");
    if git(dir, &["rev-parse", "--git-dir"]).is_none() {
        eprintln!("ERROR: git リポジトリではありません");
        return 1;
    }
    let default = ["main", "master"].into_iter().find(|b| {
        git(
            dir,
            &[
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{b}"),
            ],
        )
        .is_some()
    });
    let target = match args.first().filter(|a| !a.is_empty()) {
        Some(t) => t.clone(),
        None => crate::git::current_branch(dir),
    };
    if target.is_empty() {
        eprintln!(
            "ERROR: 対象のブランチを決められません（detached HEAD）。ブランチ名を渡してください"
        );
        return 1;
    }
    if git(
        dir,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{target}"),
        ],
    )
    .is_none()
    {
        eprintln!("ERROR: ローカルにブランチ {target} がありません");
        return 1;
    }
    if target == "main" || target == "master" {
        eprintln!("ERROR: 既定ブランチ {target} 自身はマージの対象にしません");
        return 1;
    }
    let Some(remotes) = git(dir, &["remote", "-v"]) else {
        return failed("git remote -v");
    };
    if remotes.contains("github.com") {
        eprintln!(
            "ERROR: GitHub リモートがあります。/gh-pr-create と /gh-pr-merge の流れを使ってください"
        );
        return 1;
    }

    let Some(worktrees) = git(dir, &["worktree", "list", "--porcelain"]) else {
        return failed("git worktree list");
    };
    let worktree_of = |branch: &str| -> Option<String> {
        let want = format!("refs/heads/{branch}");
        let mut path = None;
        let mut found = None;
        for line in worktrees.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                path = Some(p.to_string());
            } else if line.strip_prefix("branch ") == Some(want.as_str()) {
                found = path.clone();
            }
        }
        found
    };
    let target_wt = worktree_of(&target);
    let default_wt = default.and_then(worktree_of);
    for wt in [&target_wt, &default_wt].into_iter().flatten() {
        let Some(status) = git(Path::new(wt), &["status", "--porcelain"]) else {
            return failed("git status");
        };
        if !status.is_empty() {
            eprintln!(
                "ERROR: {wt} に未コミットの変更があります。/git-commit でコミットしてください"
            );
            return 1;
        }
    }

    // ブランチは refs/heads/ で指す（同じ名前のタグがあると、短い名前はタグを指す）
    let tip = format!("refs/heads/{target}");
    let mut out = String::new();
    out.push_str(&format!("TARGET: {target}\n"));
    match default {
        None => {
            out.push_str("DEFAULT: none\n");
            let Some(roots) = git(
                dir,
                &["rev-list", "--max-parents=0", "--abbrev-commit", &tip],
            ) else {
                return failed("git rev-list");
            };
            let roots: Vec<&str> = roots.lines().collect();
            out.push_str(&format!("ROOT_COMMITS: {}\n", roots.join(" ")));
        }
        Some(d) => {
            out.push_str(&format!("DEFAULT: {d}\n"));
            let count = |range: String| git(dir, &["rev-list", "--count", &range]);
            let (Some(commits), Some(behind)) = (
                count(format!("refs/heads/{d}..{tip}")),
                count(format!("{tip}..refs/heads/{d}")),
            ) else {
                return failed("git rev-list --count");
            };
            out.push_str(&format!("COMMITS: {commits}\nBEHIND: {behind}\n"));
        }
    }

    // 対象の履歴に先端が含まれ、既定のブランチには入っていない別のブランチ（先端が同じものは積みと見なさない）
    let Some(target_tip) = git(dir, &["rev-parse", &tip]) else {
        return failed("git rev-parse");
    };
    let Some(branches) = git(
        dir,
        &["for-each-ref", "--format=%(refname:lstrip=2)", "refs/heads"],
    ) else {
        return failed("git for-each-ref");
    };
    let is_ancestor = |a: &str, b: &str| git(dir, &["merge-base", "--is-ancestor", a, b]).is_some();
    let mut stacked = Vec::new();
    for b in branches.lines() {
        if b == target || Some(b) == default {
            continue;
        }
        let Some(b_tip) = git(dir, &["rev-parse", &format!("refs/heads/{b}")]) else {
            return failed("git rev-parse");
        };
        if b_tip == target_tip || !is_ancestor(&b_tip, &target_tip) {
            continue;
        }
        if default.is_some_and(|d| is_ancestor(&b_tip, &format!("refs/heads/{d}"))) {
            continue;
        }
        stacked.push(b.to_string());
    }
    out.push_str(&format!(
        "STACKED_ON: {}\n",
        if stacked.is_empty() {
            "none".to_string()
        } else {
            stacked.join(",")
        }
    ));
    out.push_str(&format!(
        "TARGET_WORKTREE: {}\n",
        target_wt.as_deref().unwrap_or("none")
    ));
    out.push_str(&format!(
        "DEFAULT_WORKTREE: {}\n",
        default_wt.as_deref().unwrap_or("none")
    ));

    // ls-tree はパスが無ければ空を出して 0 で終わり、木を読めなければ 0 以外で終わる（ADR の無いリポジトリと取り違えない）。
    // ls-tree は cwd で絞るので --full-tree を付ける（サブディレクトリから実行しても同じ木を見る）
    let Some(entry) = git(dir, &["ls-tree", "--full-tree", &tip, "docs/adr"]) else {
        return failed("git ls-tree");
    };
    if entry.split_whitespace().nth(1) == Some("tree") {
        let range = match default {
            Some(d) => format!("refs/heads/{d}..{tip}"),
            None => tip.clone(),
        };
        // log.showSignature は署名の検証の行を件名の前に出すので切る。-z の NUL でコミットを区切る
        let Some(log) = git(
            dir,
            &[
                "-c",
                "log.showSignature=false",
                "log",
                "-z",
                "--format=%B",
                &range,
                "--",
            ],
        ) else {
            return failed("git log");
        };
        // Refs: の値はカンマで区切った #NNNN と ADR-NNNN の並びで、ほかの書き方（「nixfiles の ADR-NNNN」）は別の
        // リポジトリの記録なので数えない
        let refs: BTreeSet<String> = log
            .split('\0')
            .filter(|msg| FEAT_FIX.is_match(msg.lines().next().unwrap_or("")))
            .flat_map(|msg| {
                msg.lines()
                    .filter_map(|l| l.strip_prefix("Refs:"))
                    .collect::<Vec<_>>()
            })
            .flat_map(|v| v.split(','))
            .map(|v| v.trim().to_string())
            .filter(|v| ADR_REF.is_match(v))
            .collect();
        let Some(names) = git(
            dir,
            &[
                "-c",
                "core.quotePath=false",
                "ls-tree",
                "--full-tree",
                "--name-only",
                &format!("{tip}:docs/adr"),
            ],
        ) else {
            return failed("git ls-tree");
        };
        let mut proposed = Vec::new();
        for r in &refs {
            let prefix = format!("{}-", &r["ADR-".len()..]);
            for name in names
                .lines()
                .filter(|n| n.starts_with(&prefix) && n.ends_with(".md"))
            {
                let Some(text) = git(dir, &["show", &format!("{tip}:docs/adr/{name}")]) else {
                    return failed("git show");
                };
                if design_records::status(&text).as_deref() == Some("proposed") {
                    // 番号の重複は design-records check が止める。ここでは 1 度だけ出す
                    proposed.push(r.clone());
                    break;
                }
            }
        }
        out.push_str(&format!(
            "PROPOSED_ADRS: {}\n",
            if proposed.is_empty() {
                "none".to_string()
            } else {
                proposed.join(" ")
            }
        ));
    }
    print!("{out}");
    0
}
