//! `review-level.sh [branch] [--base BASE]`: ローカルの /code-review に渡すレベルと対象を決める。branch を省けば今の
//! ブランチ、BASE を省けば既定のブランチ（origin の側がローカル以上に新しければ origin の側。ほかの PR がマージされた
//! 直後はローカルが古く、比較元にするとその PR の変更が差分に混ざる）。積み重なったブランチは土台のブランチを渡す。
//!
//! 出力:
//! - `BASE: <比較元>`、`TARGET: <対象のブランチ>`
//! - `WORKTREE: <パス | none>`: TARGET をチェックアウトしている worktree。/code-review の対象に渡す。ブランチの名前で
//!   渡すと /code-review は cwd のリポジトリでブランチを解くので、cwd が別のリポジトリ（や main の worktree）だと対象を
//!   見つけられず、「指摘なし」と同じ形の空の結果が返る。none はどこにもチェックアウトされていない
//! - `FILES: <変更ファイルの数>`
//! - `LEVEL: medium | high`: 差分のどのファイルも下の「文書」なら medium、ほかは high。指摘の上限が高いと文書の周ごとに
//!   8〜10 件が出て、指摘の数が収束の目安にならない
//!
//! 「文書」は、凍結せずマージの後も直せるので、周を重ねて取り切るより後で直す方が安いもの:
//! - docs/ の下の、ADR（docs/adr/NNNN-*.md）でない .md（issue、計画、説明の文書）
//! - 規則の文書: どこかの .claude/ の下の .md（スキルの文書、hook の一覧）と、どこにある CLAUDE.md と SKILL.md も
//!   （check-merge-review の is_plain_document が挙動に数える文書と同じ）
//! - 既存の ADR の「## 変更」の節より後だけを変えるもの（凍結する節やフロントマターを変えるもの、新しい ADR、消す ADR、
//!   「## 変更」の節を足す ADR は入らない）
//!
//! docs/ の外の .md（検証の記録の数値など）とスキルの中のスクリプトは文書に数えない。移動は前のパスも数える
//! （--no-renames。src/ から docs/ への移動は、コードを消す変更なので文書だけの変更として扱わない）。docs/ の判定は
//! pre-merge-check と decide-next の docs/ の前方一致と違い、ADR を除く（それらは Copilot の自動レビューの扱いで、
//! ローカルのレベルとは別の判定）。差分が空か ref を解決できなければ 1

use crate::git::git;
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

static ADR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^docs/adr/[0-9]{4}-.*\.md$").unwrap());
static CHANGE_HEADING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^## 変更[[:space:]]*$").unwrap());
static HUNK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^@@ -([0-9]+)(?:,([0-9]+))? \+([0-9]+)(?:,([0-9]+))? @@").unwrap()
});

pub fn main(args: &[String]) -> i32 {
    let mut base: Option<String> = None;
    let mut target: Option<String> = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--base" => match rest.next().filter(|v| !v.is_empty()) {
                Some(v) => base = Some(v.clone()),
                None => {
                    eprintln!("ERROR: --base には比較元が必要です");
                    return 1;
                }
            },
            a if a.starts_with('-') => {
                eprintln!("ERROR: 不明なオプション: {a}");
                return 1;
            }
            a => target = Some(a.to_string()),
        }
    }
    let dir = Path::new(".");
    let base = match base {
        Some(base) => base,
        None => {
            let Some(found) = crate::git::default_branch(dir) else {
                eprintln!(
                    "ERROR: 既定ブランチが見つかりません。--base で比べる起点を指定してください"
                );
                return 1;
            };
            default_base(dir, crate::git::local_name(&found))
        }
    };
    let target = match target {
        Some(target) => target,
        None => {
            let current = crate::git::current_branch(dir);
            if current.is_empty() {
                eprintln!("ERROR: ブランチ名を渡してください（HEAD がブランチではありません）");
                return 1;
            }
            current
        }
    };
    for r in [&base, &target] {
        if git(
            dir,
            &["rev-parse", "--verify", "-q", &format!("{r}^{{commit}}")],
        )
        .is_none()
        {
            eprintln!("ERROR: {r} を解決できません");
            return 1;
        }
    }
    let files = git(
        dir,
        &[
            "diff",
            "--no-renames",
            "--name-only",
            &format!("{base}...{target}"),
        ],
    )
    .unwrap_or_default();
    let files: Vec<&str> = files.lines().filter(|l| !l.is_empty()).collect();
    if files.is_empty() {
        eprintln!(
            "ERROR: {base} と {target} の差分が空です。ブランチが別の worktree にあるなら、cwd ではなくブランチ名で渡してください"
        );
        return 1;
    }
    let merge_base = git(dir, &["merge-base", &base, &target]).unwrap_or_default();
    let level = if files
        .iter()
        .all(|path| is_document(dir, &merge_base, &target, path))
    {
        "medium"
    } else {
        "high"
    };
    println!("BASE: {base}");
    println!("TARGET: {target}");
    println!(
        "WORKTREE: {}",
        worktree_of(dir, &target).unwrap_or_else(|| "none".to_string())
    );
    println!("FILES: {}", files.len());
    println!("LEVEL: {level}");
    0
}

/// 既定のブランチの比較元。origin の側がローカル以上に新しければ origin の側を使う（ローカルが先に進んでいる
/// git-merge の流れではローカル。ローカルが無ければ origin の側）
fn default_base(dir: &Path, name: &str) -> String {
    let remote = format!("origin/{name}");
    let has_remote = git(
        dir,
        &[
            "rev-parse",
            "--verify",
            "-q",
            &format!("refs/remotes/{remote}"),
        ],
    )
    .is_some();
    if has_remote {
        let has_local = git(
            dir,
            &["rev-parse", "--verify", "-q", &format!("refs/heads/{name}")],
        )
        .is_some();
        if !has_local || git(dir, &["merge-base", "--is-ancestor", name, &remote]).is_some() {
            return remote;
        }
    }
    name.to_string()
}

fn is_document(dir: &Path, merge_base: &str, target: &str, path: &str) -> bool {
    if ADR.is_match(path) {
        return adr_change_section_only(dir, merge_base, target, path);
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    (path.starts_with("docs/") && path.ends_with(".md"))
        || ((path.starts_with(".claude/") || path.contains("/.claude/")) && path.ends_with(".md"))
        || name == "CLAUDE.md"
        || name == "SKILL.md"
}

/// ADR の「## 変更」の見出しの行の番号（1 から）
fn change_heading_line(dir: &Path, rev: &str, path: &str) -> Option<usize> {
    let text = git(dir, &["show", &format!("{rev}:{path}")])?;
    text.lines()
        .position(|l| CHANGE_HEADING.is_match(l))
        .map(|i| i + 1)
}

/// 既存の ADR の変更が「## 変更」の見出しより後だけか。-U0 の hunk の見出し `@@ -a[,b] +c[,d] @@` で見る。行数が 0 の
/// 側（足すだけ、消すだけ）は「その行の後」を表すので、見出しの行そのものも許す
fn adr_change_section_only(dir: &Path, merge_base: &str, target: &str, path: &str) -> bool {
    for rev in [merge_base, target] {
        if git(dir, &["cat-file", "-e", &format!("{rev}:{path}")]).is_none() {
            return false;
        }
    }
    let (Some(old_heading), Some(new_heading)) = (
        change_heading_line(dir, merge_base, path),
        change_heading_line(dir, target, path),
    ) else {
        return false;
    };
    let Some(diff) = git(
        dir,
        &[
            "diff",
            "-U0",
            "--no-renames",
            merge_base,
            target,
            "--",
            path,
        ],
    ) else {
        return false;
    };
    let after = |start: usize, count: usize, heading: usize| {
        if count == 0 {
            start >= heading
        } else {
            start > heading
        }
    };
    diff.lines().filter_map(|l| HUNK.captures(l)).all(|c| {
        let num = |i: usize, default: usize| {
            c.get(i)
                .map_or(default, |m| m.as_str().parse().unwrap_or(0))
        };
        after(num(1, 0), num(2, 1), old_heading) && after(num(3, 0), num(4, 1), new_heading)
    })
}

/// target をチェックアウトしている worktree のパス。ディレクトリが消えた worktree（prunable）は数えない
fn worktree_of(dir: &Path, target: &str) -> Option<String> {
    let list = git(dir, &["worktree", "list", "--porcelain"])?;
    let want = format!("refs/heads/{target}");
    list.split("\n\n").find_map(|record| {
        let mut path = None;
        let (mut hit, mut gone) = (false, false);
        for line in record.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                path = Some(p.to_string());
            } else if line.strip_prefix("branch ") == Some(want.as_str()) {
                hit = true;
            } else if line == "prunable" || line.starts_with("prunable ") {
                gone = true;
            }
        }
        (hit && !gone).then_some(path).flatten()
    })
}
