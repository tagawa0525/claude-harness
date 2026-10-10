//! ADR（docs/adr/ 直下の NNNN-*.md）の判定と、コミット時の節の検査（`claude-hooks adr-sections`）
//!
//! 節は MADR に倣う: 背景（Context）・検討した案（Considered Options）・決定と理由（Decision
//! Outcome）・帰結（Consequences）・確認（Confirmation）。背景と決定と理由は必須。ホストの状態や
//! 再開手順のような節を足させない（ADR-0005）。中身が決定か状態かは判定できないのでレビューに任せる。
//! 節の一覧は .claude/skills/adr/SKILL.md と揃える。
//! index が確定するのはコミットの時なので、PreToolUse ではなく git の pre-commit
//! （modules/home/parts/git.nix）から呼ぶ。git add && git commit や git commit -a も捕まえる

use crate::git;
use std::path::Path;

/// ADR のファイルか（docs/adr/ 直下の NNNN-*.md。4 桁の番号とハイフンで始まる）。
/// README やテンプレートは含めない。.claude/scripts/branch-topics.sh も同じ条件で数える
pub fn is_adr(path: &str) -> bool {
    path.strip_prefix("docs/adr/").is_some_and(|name| {
        let b = name.as_bytes();
        !name.contains('/')
            && name.ends_with(".md")
            && b.len() > 5
            && b[..4].iter().all(u8::is_ascii_digit)
            && b[4] == b'-'
    })
}

const ALLOWED: &[&str] = &["背景", "検討した案", "決定と理由", "帰結", "確認"];
const REQUIRED: &[&str] = &["背景", "決定と理由"];

/// ステージ済みの ADR の節を検査する。問題があれば理由を返す（git の pre-commit が表示して止める）
pub fn check_staged(dir: &Path) -> Option<String> {
    // -z: 日本語などのパスを引用符で囲ませない（core.quotePath）
    let staged = git::git(
        dir,
        &["diff", "--cached", "--name-only", "-z", "--diff-filter=AM"],
    )?;
    let adrs: Vec<&str> = staged.split('\0').filter(|p| is_adr(p)).collect();
    if adrs.is_empty() || !git::own_project(dir) {
        return None;
    }
    let problems: Vec<String> = adrs
        .iter()
        .filter_map(|p| {
            let body = git::git(dir, &["show", &format!(":{p}")])?;
            problem(p, &headings(&body))
        })
        .collect();
    if problems.is_empty() {
        return None;
    }
    Some(format!(
        "ADR の節は一般的な形に限ります: {}（{} は必須）。/adr スキルを読んで直してください。\n{}",
        ALLOWED.join("・"),
        REQUIRED.join("・"),
        problems.join("\n")
    ))
}

/// `## ` の見出し（コードブロックの中は除く）
fn headings(body: &str) -> Vec<String> {
    let mut in_fence = false;
    let mut out = Vec::new();
    for line in body.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence && let Some(h) = line.strip_prefix("## ") {
            out.push(h.trim().to_string());
        }
    }
    out
}

fn problem(path: &str, hs: &[String]) -> Option<String> {
    let extra: Vec<&str> = hs
        .iter()
        .map(String::as_str)
        .filter(|h| !ALLOWED.contains(h))
        .collect();
    let missing: Vec<&str> = REQUIRED
        .iter()
        .copied()
        .filter(|r| !hs.iter().any(|h| h == r))
        .collect();
    if extra.is_empty() && missing.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    if !extra.is_empty() {
        parts.push(format!("決まっていない節「{}」", extra.join("」「")));
    }
    if !missing.is_empty() {
        parts.push(format!("足りない節「{}」", missing.join("」「")));
    }
    Some(format!("- {path}: {}", parts.join("、")))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
