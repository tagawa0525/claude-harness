//! ADR（docs/adr/ 直下の NNNN-*.md）の判定と、コミット時の節の検査（`claude-hooks adr-sections`）
//!
//! 検査するのは MADR 4.0.0 の必須の節があるかだけ（ADR-0005）。見出しは日本語（スキルの対応表）でも
//! MADR の原文の英語でもよい。任意の節や MADR に無い節は止めない: MADR は節を足すことを禁じておらず、
//! 中身（決定か状態か）の判断はレビューの役目。推奨の節（帰結・確認）が無ければ警告だけ出す。
//! 推奨は MADR の minimal テンプレート（必須 + 帰結）と、テンプレートの注記（確認は「任意だが多くの
//! ADR が含める」）に基づく。見出しの深さは問わない（MADR では帰結と確認は決定と理由の下の ###）。
//! 対応表は .claude/skills/adr/SKILL.md と揃える。
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

/// MADR 4.0.0 の必須の節（日本語の見出し, 英語の見出し）
const REQUIRED: &[(&str, &str)] = &[
    ("背景", "Context and Problem Statement"),
    ("検討した案", "Considered Options"),
    ("決定と理由", "Decision Outcome"),
];

/// MADR 4.0.0 の推奨の節（日本語の見出し, 英語の見出し）。無ければ警告だけ出す
const RECOMMENDED: &[(&str, &str)] = &[("帰結", "Consequences"), ("確認", "Confirmation")];

/// 検査の結果。error があればコミットを止め、warning は表示だけする
#[derive(Default)]
pub struct Report {
    pub error: Option<String>,
    pub warning: Option<String>,
}

/// ステージ済みの ADR の節を検査する（git の pre-commit が表示し、error があれば止める）
pub fn check_staged(dir: &Path) -> Report {
    let mut report = Report::default();
    let Some((errors, warnings)) = collect(dir) else {
        return report;
    };
    if !errors.is_empty() {
        report.error = Some(format!(
            "ADR に MADR の必須の節（{}）がありません。/adr スキルのテンプレートで直してください。\n{}",
            names(REQUIRED),
            errors.join("\n")
        ));
    }
    if !warnings.is_empty() {
        report.warning = Some(format!(
            "ADR に推奨の節（{}）がありません。要らないと判断したのでなければ足してください（/adr）。\n{}",
            names(RECOMMENDED),
            warnings.join("\n")
        ));
    }
    report
}

fn names(sections: &[(&str, &str)]) -> String {
    sections
        .iter()
        .map(|(ja, _)| *ja)
        .collect::<Vec<_>>()
        .join("・")
}

/// ADR ごとの、足りない必須の節と推奨の節の行
fn collect(dir: &Path) -> Option<(Vec<String>, Vec<String>)> {
    // -z: 日本語などのパスを引用符で囲ませない（core.quotePath）
    let staged = git::git(
        dir,
        &["diff", "--cached", "--name-only", "-z", "--diff-filter=AM"],
    )?;
    let adrs: Vec<&str> = staged.split('\0').filter(|p| is_adr(p)).collect();
    if adrs.is_empty() || !git::own_project(dir) {
        return None;
    }
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for p in adrs {
        let Some(body) = git::git(dir, &["show", &format!(":{p}")]) else {
            continue;
        };
        let hs = headings(&body);
        if let Some(line) = missing(p, REQUIRED, &hs) {
            errors.push(line);
        }
        if let Some(line) = missing(p, RECOMMENDED, &hs) {
            warnings.push(line);
        }
    }
    Some((errors, warnings))
}

/// `##` 以下の見出し（コードブロックの中は除く）
fn headings(body: &str) -> Vec<String> {
    let mut in_fence = false;
    let mut out = Vec::new();
    for line in body.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence
            && let Some(rest) = line.strip_prefix("##")
            && let Some(h) = rest.trim_start_matches('#').strip_prefix(' ')
        {
            out.push(h.trim().to_string());
        }
    }
    out
}

/// 足りない節があれば `- <path>: 足りない節「…」` の行
fn missing(path: &str, sections: &[(&str, &str)], hs: &[String]) -> Option<String> {
    let lacking: Vec<String> = sections
        .iter()
        .filter(|(ja, en)| !hs.iter().any(|h| h == ja || h == en))
        .map(|(ja, en)| format!("{ja}（{en}）"))
        .collect();
    if lacking.is_empty() {
        return None;
    }
    Some(format!("- {path}: 足りない節「{}」", lacking.join("」「")))
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
