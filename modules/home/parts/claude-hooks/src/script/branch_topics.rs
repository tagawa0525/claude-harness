//! `branch-topics.sh [base [head]]`: ブランチが含むトピックを要約する（1 ブランチ 1 トピックの確認用）。
//!
//! base を省くと既定のブランチ（origin/HEAD → origin/main → origin/master → main → master の最初にあるもの。
//! git.rs の default_branch）と比べる。変更は merge-base から HEAD まで（base...HEAD）を見るので、分岐した後に base が
//! 進んでいても混ざらない。head を渡すと HEAD の代わりにそのコミットを要約する（チェックアウトしていない PR のブランチを
//! 見るとき。例: origin/main origin/feat/x）。
//!
//! ADR と issue は docs/adr/・docs/issues/ の直下にある NNNN-name.md（4 桁の番号とハイフンで始まる）。README などは
//! 含めない。status はフロントマターの `status:` の値（design-records の読み方）。ディレクトリが無いリポジトリでは該当の
//! 行を出さない。
//!
//! 出力（行が無いものは省く）:
//! - `BASE: <ref>`、`HEAD: <ref>`、`COMMITS: <n>`
//! - `TYPES: <type>=<n> ...`（件数の多い順、同数は名前の順。Conventional Commits の型が無い件名は other）
//! - `ADR_NEW: <path> (<status>)`、`ADR_STATUS: <path> <old> -> <new>`（既存の ADR の status の変化）
//! - `ISSUE_NEW: ...`、`ISSUE_STATUS: ...`
//! - `DIRS: <dir> ...`（変えたファイルのディレクトリの先頭 2 段。リポジトリの直下は .）

use crate::git::git;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::LazyLock;

const USAGE: &str = "Usage: branch-topics.sh [base [head]]";
static TYPE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([a-z]+)(\([^)]*\))?!?: ").unwrap());

pub fn main(args: &[String]) -> i32 {
    let dir = Path::new(".");
    if git(dir, &["rev-parse", "--git-dir"]).is_none() {
        for notice in crate::output::take_notices() {
            eprintln!("{notice}");
        }
        eprintln!("ERROR: git リポジトリではありません");
        return 1;
    }
    let base = match args {
        [] => match crate::git::default_branch(dir) {
            Some(base) => base,
            None => {
                eprintln!(
                    "ERROR: 既定ブランチが見つかりません。比べる起点を引数で指定してください"
                );
                return 1;
            }
        },
        [base] | [base, _] => base.clone(),
        _ => {
            eprintln!("{USAGE}");
            return 1;
        }
    };
    let tip = args.get(1).cloned().unwrap_or_else(|| "HEAD".to_string());
    for r in [&base, &tip] {
        if git(
            dir,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{r}^{{commit}}"),
            ],
        )
        .is_none()
        {
            eprintln!("ERROR: コミットが見つかりません: {r}");
            return 1;
        }
    }
    let Some(out) = crate::git::run(dir, &["merge-base", &base, &tip], None) else {
        return 1;
    };
    if !out.status.success() {
        eprint!("{}", String::from_utf8_lossy(&out.stderr));
        return out.status.code().unwrap_or(1);
    }
    let fork = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let range = format!("{base}..{tip}");

    println!("BASE: {base}");
    println!("HEAD: {tip}");
    println!(
        "COMMITS: {}",
        git(dir, &["rev-list", "--count", &range])
            .unwrap_or_default()
            .trim()
    );

    let mut types: BTreeMap<String, u64> = BTreeMap::new();
    for subject in git(dir, &["log", "--format=%s", &range])
        .unwrap_or_default()
        .lines()
    {
        let kind = TYPE
            .captures(subject)
            .map_or("other".to_string(), |c| c[1].to_string());
        *types.entry(kind).or_default() += 1;
    }
    if !types.is_empty() {
        let mut types: Vec<(String, u64)> = types.into_iter().collect();
        // 件数の多い順、同数は名前の順（BTreeMap の順を保つ安定な並べ替え）
        types.sort_by_key(|t| std::cmp::Reverse(t.1));
        let line: Vec<String> = types.iter().map(|(t, n)| format!("{t}={n}")).collect();
        println!("TYPES: {}", line.join(" "));
    }

    report(dir, &fork, &tip, "docs/adr", "ADR");
    report(dir, &fork, &tip, "docs/issues", "ISSUE");

    let dirs: BTreeSet<String> = git(
        dir,
        &[
            "-c",
            "core.quotePath=false",
            "diff",
            "--name-only",
            &fork,
            &tip,
        ],
    )
    .unwrap_or_default()
    .lines()
    .filter(|l| !l.is_empty())
    .map(|path| {
        let parts: Vec<&str> = path.split('/').collect();
        match parts.len() {
            1 => ".".to_string(),
            2 => parts[0].to_string(),
            _ => format!("{}/{}", parts[0], parts[1]),
        }
    })
    .collect();
    if !dirs.is_empty() {
        println!("DIRS: {}", dirs.into_iter().collect::<Vec<_>>().join(" "));
    }
    0
}

/// フロントマターの status の値（無ければ -）。フロントマターの形は design-records が持つ
fn status(dir: &Path, rev: &str, path: &str) -> String {
    git(dir, &["show", &format!("{rev}:{path}")])
        .and_then(|text| design_records::status(&text))
        .unwrap_or_else(|| "-".to_string())
}

/// dir の直下の NNNN-*.md の追加と status の変化。改名（R）は新しいファイルとして数えず（claude-hooks のマージの前の
/// 判定と同じ）、改名の前の status と比べる。改名の検出は diff.renames の設定に左右されないよう --find-renames で明示する
fn report(dir: &Path, fork: &str, tip: &str, docs: &str, prefix: &str) {
    let record = Regex::new(&format!(r"^{}/[0-9]{{4}}-[^/]*\.md$", regex::escape(docs))).unwrap();
    // 非 ASCII のパスを引用させない（NNNN-*.md の判定と出力のため）
    let Some(changes) = git(
        dir,
        &[
            "-c",
            "core.quotePath=false",
            "diff",
            "--find-renames",
            "--diff-filter=AMR",
            "--name-status",
            fork,
            tip,
            "--",
            docs,
        ],
    ) else {
        return;
    };
    for line in changes.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let (kind, src, path) = match fields.as_slice() {
            [kind, path] => (*kind, *path, *path),
            [kind, src, path] => (*kind, *src, *path),
            _ => continue,
        };
        if !record.is_match(path) {
            continue;
        }
        if kind == "A" {
            println!("{prefix}_NEW: {path} ({})", status(dir, tip, path));
        } else if kind == "M" || kind.starts_with('R') {
            let (old, new) = (status(dir, fork, src), status(dir, tip, path));
            if old != new {
                println!("{prefix}_STATUS: {path} {old} -> {new}");
            }
        }
    }
}
