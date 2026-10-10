//! check-adr-sections: コミットする ADR（docs/adr/*.md）の節を一般的な形に限る
//!
//! 節は MADR に倣う: 背景（Context）・検討した案（Considered Options）・決定と理由（Decision
//! Outcome）・帰結（Consequences）・確認（Confirmation）。背景と決定と理由は必須。
//! ホストの状態や再開手順のような節を足させない（ADR-0005）。中身が決定か状態かは
//! 判定できないので、それはレビューに任せる。他人のプロジェクトでは検査しない（git::own_project）。
//! 節の一覧は .claude/skills/adr/SKILL.md と揃える

use super::Rule;
use crate::git;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;

pub struct CheckAdrSections;

const ALLOWED: &[&str] = &["背景", "検討した案", "決定と理由", "帰結", "確認"];
const REQUIRED: &[&str] = &["背景", "決定と理由"];

impl Rule for CheckAdrSections {
    fn name(&self) -> &'static str {
        "check-adr-sections"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        for cmd in shell.commands() {
            if cmd.git_subcommand().map(|(sub, _)| sub) != Some("commit") {
                continue;
            }
            let dir = shell.target_dir(&cmd, &input.cwd);
            let staged = git::git(
                &dir,
                &["diff", "--cached", "--name-only", "--diff-filter=AM"],
            )
            .unwrap_or_default();
            let adrs: Vec<&str> = staged
                .lines()
                .filter(|p| p.starts_with("docs/adr/") && p.ends_with(".md"))
                .collect();
            if adrs.is_empty() || !git::own_project(&dir) {
                continue;
            }
            let problems: Vec<String> = adrs
                .iter()
                .filter_map(|p| {
                    let body = git::git(&dir, &["show", &format!(":{p}")])?;
                    problem(p, &headings(&body))
                })
                .collect();
            if !problems.is_empty() {
                return vec![Finding::Deny(format!(
                    "ADR の節は一般的な形に限ります: {}（{} は必須）。/adr スキルを読んで直してください。\n{}",
                    ALLOWED.join("・"),
                    REQUIRED.join("・"),
                    problems.join("\n")
                ))];
            }
        }
        Vec::new()
    }
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
