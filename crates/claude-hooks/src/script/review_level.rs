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
//! - `LEVEL: low | medium | high`: 差分のどのファイルも下の「文書」なら low。文書のほかが設定のファイル（`is_settings`）
//!   だけで、その足した行と消した行が合わせて `SMALL_SETTINGS_LINES` 以下なら medium。ほかは high。指摘の上限が高いと
//!   文書の周ごとに 8〜10 件が出て、指摘の数が収束の目安にならない（ADR-0066）
//! - `NEXT: /code-review <LEVEL> <WORKTREE>（…）`: 回す 1 行。WORKTREE が none なら対象を `<PR の URL>` にし、PR が無い
//!   ときの取り出し方を添える
//!
//! 「文書」は、凍結せずマージの後も直せるので、周を重ねて取り切るより後で直す方が安いもの:
//! - docs/ の下の、ADR（docs/adr/NNNN-*.md）でない .md（issue、計画、説明の文書）
//! - 規則の文書: どこかの .claude/ の下か、リポジトリの先頭の claude/（ハーネスが配る元）の下の .md（スキルの文書、
//!   hook の一覧）と、どこにある CLAUDE.md と SKILL.md も
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
    // 差分のパスはリポジトリの直下からなので、git もそこで動かす（サブディレクトリの cwd からの相対の pathspec で、
    // ADR の差分が空にならないように）
    let Some(top) = git(Path::new("."), &["rev-parse", "--show-toplevel"]) else {
        for notice in crate::output::take_notices() {
            eprintln!("{notice}");
        }
        eprintln!("ERROR: git のリポジトリの中で実行してください");
        return 1;
    };
    let dir = Path::new(&top);
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
    // パスと行数を 1 回の差分で読む（パスを pathspec に渡して引き直すと、`[` などの文字で別のファイルにも当たる）
    let numstat = git(
        dir,
        &[
            // ASCII でない文字のパス（日本語の名前の文書）を引用せずに読む
            "-c",
            "core.quotePath=false",
            "diff",
            "--no-renames",
            "--numstat",
            &format!("{base}...{target}"),
        ],
    )
    .unwrap_or_default();
    let changes: Vec<(&str, Option<usize>)> = numstat.lines().filter_map(parse_numstat).collect();
    let files: Vec<&str> = changes.iter().map(|(path, _)| *path).collect();
    if files.is_empty() {
        eprintln!(
            "ERROR: {base} と {target} の差分が空です。ブランチが別の worktree にあるなら、cwd ではなくブランチ名で渡してください"
        );
        return 1;
    }
    let merge_base = git(dir, &["merge-base", &base, &target]).unwrap_or_default();
    let rest: Vec<(&str, Option<usize>)> = changes
        .iter()
        .copied()
        .filter(|(path, _)| !is_document(dir, &merge_base, &target, path))
        .collect();
    // 行数の読めないファイル（バイナリ）が 1 つでもあれば合計は None になり、high に倒れる
    let settings_only = rest.iter().all(|(path, _)| is_settings(path));
    let settings_lines: Option<usize> = rest.iter().map(|(_, lines)| *lines).sum();
    let level = if rest.is_empty() {
        "low"
    } else if settings_only && settings_lines.is_some_and(|n| n <= SMALL_SETTINGS_LINES) {
        "medium"
    } else {
        "high"
    };
    let worktree = worktree_of(dir, &target);
    println!("BASE: {base}");
    println!("TARGET: {target}");
    println!("WORKTREE: {}", worktree.as_deref().unwrap_or("none"));
    println!("FILES: {}", files.len());
    println!("LEVEL: {level}");
    const ROUND: &str = "1 周目はこの LEVEL で回す。直した後の周のレベルは /git-merge の「直す指摘の基準」の「終わり」の項で決める";
    match worktree {
        Some(path) => println!(
            "NEXT: /code-review {level} {}（{ROUND}）",
            super::wait_step::quote(&path)
        ),
        None => println!(
            "NEXT: /code-review {level} <PR の URL>（{target} をチェックアウトした worktree が無い。ブランチ名は cwd のリポジトリで解かれ、空の結果が返る。PR が無ければ git worktree add で取り出して review-level を実行し直す。{ROUND}）"
        ),
    }
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
        || ((path.starts_with(".claude/")
            || path.contains("/.claude/")
            || path.starts_with("claude/"))
            && path.ends_with(".md"))
        || name == "CLAUDE.md"
        || name == "SKILL.md"
}

/// 設定の差分を medium に数える、足した行と消した行の合計の上限。設定の数行（許可の 1 行、.worktreelink の 1 行）を
/// 拾い、構成を組み替える変更は high に残す
const SMALL_SETTINGS_LINES: usize = 20;

/// ゲートもビルドも変えない設定のファイル: 配る設定（claude-settings.json）、どこかの .claude/ の下の Claude Code の
/// 設定、worktree が共有する一覧、git の無視の一覧、エディタの設定。拡張子（.json、.yaml、.toml）では見分けない。
/// .pre-commit-config.yaml、Cargo.toml、prh.yml、CI の手順のように、ゲートやビルドの挙動を変えるファイルも同じ
/// 拡張子を持つ。.gitattributes は改行の扱い（`eol=lf`）で Windows の sh の実行を変えるので入れない
fn is_settings(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let in_claude_dir = path.starts_with(".claude/") || path.contains("/.claude/");
    match name {
        "claude-settings.json" | ".worktreelink" | ".gitignore" | ".editorconfig" => true,
        "settings.json" | "settings.local.json" => in_claude_dir,
        _ => false,
    }
}

/// `git diff --numstat` の 1 行（`<足した行>\t<消した行>\t<パス>`）。バイナリの行数（`-`）は None
fn parse_numstat(line: &str) -> Option<(&str, Option<usize>)> {
    let mut cols = line.splitn(3, '\t');
    let (added, deleted, path) = (cols.next()?, cols.next()?, cols.next()?);
    let lines = added
        .parse::<usize>()
        .ok()
        .zip(deleted.parse::<usize>().ok())
        .map(|(a, d)| a + d);
    Some((path, lines))
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
            // 利用者の設定の色や外の diff で、hunk の見出しの形を変えない
            "--no-color",
            "--no-ext-diff",
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
    // hunk を 1 つも読めなければ、文書の変更とみなさない（high に倒す）
    let hunks: Vec<_> = diff.lines().filter_map(|l| HUNK.captures(l)).collect();
    !hunks.is_empty()
        && hunks.iter().all(|c| {
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
