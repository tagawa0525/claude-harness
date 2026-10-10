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

/// ステージ済みの ADR の節を検査する（git の pre-commit が表示し、error があれば止める）。
/// 必須の節で止めるのは新しく加える ADR だけ。既存の ADR の変更（確定した ADR の前付けの更新、
/// 別の形式で書いた古い ADR の直しなど）は警告にとどめる
pub fn check_staged(dir: &Path) -> Report {
    let mut report = Report::default();
    let Some(found) = collect(dir) else {
        return report;
    };
    if !found.added.is_empty() {
        report.error = Some(format!(
            "ADR に MADR の必須の節（{}）がありません。/adr スキルのテンプレートで直してください。\n{}",
            names(REQUIRED),
            found.added.join("\n")
        ));
    }
    let mut warnings = Vec::new();
    if !found.modified.is_empty() {
        warnings.push(format!(
            "変更した既存の ADR に MADR の必須の節（{}）がありません。新しい ADR ではないのでコミットは止めません。\n{}",
            names(REQUIRED),
            found.modified.join("\n")
        ));
    }
    if !found.recommended.is_empty() {
        warnings.push(format!(
            "ADR に推奨の節（{}）がありません。要らないと判断したのでなければ足してください（/adr）。\n{}",
            names(RECOMMENDED),
            found.recommended.join("\n")
        ));
    }
    if !warnings.is_empty() {
        report.warning = Some(warnings.join("\n"));
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

/// 足りない節の行。added は新しい ADR の必須の節、modified は既存の ADR の必須の節、
/// recommended は推奨の節
#[derive(Default)]
struct Found {
    added: Vec<String>,
    modified: Vec<String>,
    recommended: Vec<String>,
}

fn collect(dir: &Path) -> Option<Found> {
    // -z: 日本語などのパスを引用符で囲ませない（core.quotePath）。
    // --no-renames: 改名を「消して加えた」と数え、改名で入った ADR も新しい ADR として検査する
    let staged = git::git(
        dir,
        &[
            "diff",
            "--cached",
            "--name-status",
            "-z",
            "--no-renames",
            "--diff-filter=AM",
        ],
    )?;
    let fields: Vec<&str> = staged.split('\0').filter(|f| !f.is_empty()).collect();
    let adrs: Vec<(&str, &str)> = fields
        .chunks(2)
        .filter_map(|c| match c {
            [status, path] if is_adr(path) => Some((*status, *path)),
            _ => None,
        })
        .collect();
    if adrs.is_empty() || !git::own_project(dir) {
        return None;
    }
    let mut found = Found::default();
    for (status, p) in adrs {
        let Some(body) = git::git(dir, &["show", &format!(":{p}")]) else {
            continue;
        };
        let hs = headings(&body);
        if let Some(line) = missing(p, REQUIRED, &hs) {
            if status == "A" {
                found.added.push(line);
            } else {
                found.modified.push(line);
            }
        }
        if let Some(line) = missing(p, RECOMMENDED, &hs) {
            found.recommended.push(line);
        }
    }
    Some(found)
}

/// `##` 以下の見出し（コードブロックの中は除く）。コードブロックは、開いた記号（``` か ~~~）と
/// 同じ記号を同じ数以上並べた行で閉じる。見出しの閉じの `#`（`## 背景 ##`）は外す
fn headings(body: &str) -> Vec<String> {
    let mut fence: Option<(char, usize)> = None;
    let mut out = Vec::new();
    for line in body.lines() {
        let t = line.trim_start();
        if let Some(c) = t.chars().next().filter(|c| *c == '`' || *c == '~') {
            let n = t.chars().take_while(|x| *x == c).count();
            if n >= 3 {
                match fence {
                    None => fence = Some((c, n)),
                    Some((open, len)) if open == c && n >= len && t[n..].trim().is_empty() => {
                        fence = None
                    }
                    Some(_) => {}
                }
                continue;
            }
        }
        if fence.is_some() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("##")
            && let Some(h) = rest.trim_start_matches('#').strip_prefix(' ')
        {
            let h = h.trim_end();
            let closed = h.trim_end_matches('#');
            let h = if closed.len() < h.len() && (closed.is_empty() || closed.ends_with(' ')) {
                closed.trim_end()
            } else {
                h
            };
            out.push(h.to_string());
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
