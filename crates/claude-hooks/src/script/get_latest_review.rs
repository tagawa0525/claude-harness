//! `get-latest-review.sh <PR の URL>`: 最新の Copilot のレビューの要約をテキストで出す（/gh-pr-review の Step 2。
//! decide-next.sh も読む）。
//!
//! 出力:
//! - `ROUND: <n>`: PR の上の Copilot のレビューの件数（周回の数そのものではない。decide-next.sh がレビューの要求の件数と
//!   突き合わせて周回を決める）。0 なら `NOTE:` の行だけを続けて終える
//! - `REVIEW_ID`、`STATE`、`HEADLINE`（本文の 1 行目）
//! - `REVIEW_FAILED: yes|no`: Copilot がレビューできずに終わったレビューか
//! - `SUCCESSFUL_REVIEWS: <n>`: 失敗していない Copilot のレビューの件数（文書だけの PR は、一度でも成功していれば後の
//!   レビューが失敗しても止めない）
//! - `INLINE_COMMENTS: <n>`: このレビューに紐づくインラインコメントの数（通常の指摘）
//! - `SUPPRESSED_COMMENTS: <n>`
//! - `SEVERITY: v2|unknown`: 本文が重大度を持つ形（v2）か。unknown なら `REMAINING_*` は出さない
//! - `REMAINING_HIGH|MEDIUM|LOW: <n>`: 残っている指摘（開いている指摘と Previously missed）
//! - `PREVIOUSLY_MISSED: <n>`: Previously missed の指摘の数（v2 の本文なら、重大度が分からなくても出す）
//! - `--- suppressed ---` の後に Suppressed comments の節の本文、`--- previously missed ---` の後に Previously missed の
//!   節の本文
//!
//! Suppressed comments は Copilot が低確度と判断した指摘で、インラインのスレッドにならずレビューの本文の `<details>` の
//! 中にだけ現れる。get-review-comments.sh では取れないので、ここで読む。Copilot はレビューできなかったときも本文だけの
//! レビューを提出する（"Copilot wasn't able to review any files in this pull request." など）。インラインの指摘は 0 件
//! なので、区別しないと「指摘なし」と同じ扱いになる

use super::pr_url;
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

const USAGE: &str = "Usage: get-latest-review.sh <PR の URL>";
const V2_MARK: &str = "<!-- ccr-overview-v2 -->";

/// レビューの失敗は本文の 1 行目に出る（インラインの指摘もサマリーも無い）
static FAILED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(wasn'?t able to review|unable to review|encountered an error)").unwrap()
});
static VERDICT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^### .*(Approval recommended|Changes recommended|Needs a closer look)")
        .unwrap()
});
static SUPPRESSED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Suppressed comments \(([0-9]+)\)").unwrap());
static SEVERITY_ICON: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"alt="[^"]* severity""#).unwrap());
static HEADING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^.*<summary><strong>[0-9 ]*").unwrap());
static ITEM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[[:space:]]*- ").unwrap());
static FILES_REVIEWED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^- \*\*Files reviewed:").unwrap());

pub fn main(args: &[String]) -> i32 {
    let Some(pr) = pr_url::or_usage(
        pr_url::parse(args.first().map(String::as_str).unwrap_or("")),
        USAGE,
    ) else {
        return 1;
    };
    if args.len() != 1 {
        eprintln!("{USAGE}");
        return 1;
    }
    match summary(&pr) {
        Ok(text) => {
            print!("{text}");
            0
        }
        Err(code) => code,
    }
}

/// 要約のテキスト（モジュールの説明の出力）。gh が失敗すれば、その終了コード（理由は標準エラーに出す）
pub fn summary(pr: &pr_url::Pr) -> Result<String, i32> {
    let mut out = String::new();
    macro_rules! out_line {
        ($($t:tt)*) => {{
            out.push_str(&format!($($t)*));
            out.push('\n');
        }};
    }
    // Copilot のレビューを提出の順に全件（周回の数に全件が要る）。--paginate に配列を返す --jq を渡すと、ページごとの
    // 配列がつながって不正な JSON になるので、要素を 1 つずつ出させて読む
    let stdout = crate::gh::output_for_repo(
        &pr.repo,
        &[
            "api",
            "--paginate",
            &format!(
                "repos/{{owner}}/{{repo}}/pulls/{}/reviews?per_page=100",
                pr.number
            ),
            "--jq",
            r#".[] | select(.user.login == "copilot-pull-request-reviewer[bot]")"#,
        ],
    )?;
    let reviews: Result<Vec<Value>, _> = serde_json::Deserializer::from_slice(&stdout)
        .into_iter::<Value>()
        .collect();
    let Some(reviews) = reviews
        .ok()
        .filter(|r| r.iter().all(|review| review["id"].is_u64()))
    else {
        eprintln!(
            "ERROR: gh api の出力がレビューの並びでない: {}",
            String::from_utf8_lossy(&stdout)
        );
        return Err(1);
    };
    let Some(latest) = reviews.last() else {
        out_line!("ROUND: 0");
        out_line!("NOTE: Copilot のレビューはまだありません");
        return Ok(out);
    };
    let review_id = latest["id"].as_u64().unwrap_or_default();
    let inline = {
        let stdout = crate::gh::output_for_repo(
            &pr.repo,
            &[
                "api",
                "--paginate",
                &format!(
                    "repos/{{owner}}/{{repo}}/pulls/{}/reviews/{review_id}/comments?per_page=100",
                    pr.number
                ),
                "--jq",
                ".[].id",
            ],
        )?;
        stdout.iter().filter(|&&b| b == b'\n').count()
    };
    let body = body_of(latest);
    let lines: Vec<&str> = body.split('\n').collect();
    let suppressed = suppressed_section(&lines);
    let suppressed_count = SUPPRESSED
        .captures(&suppressed.join("\n"))
        .map_or("0".to_string(), |c| c[1].to_string());
    let v2 = body.contains(V2_MARK);
    let sections = v2.then(|| Sections::read(&lines));
    let severity_known = sections.as_ref().is_some_and(|s| {
        s.items.iter().all(|(_, level)| level.is_some()) && VERDICT.is_match(&body)
    });
    let successful = reviews
        .iter()
        .filter(|review| !FAILED.is_match(first_line(&body_of(review))))
        .count();

    out_line!("ROUND: {}", reviews.len());
    out_line!("REVIEW_ID: {review_id}");
    out_line!("STATE: {}", text(&latest["state"]));
    out_line!("HEADLINE: {}", first_line(&body));
    out_line!(
        "REVIEW_FAILED: {}",
        if FAILED.is_match(first_line(&body)) {
            "yes"
        } else {
            "no"
        }
    );
    out_line!("SUCCESSFUL_REVIEWS: {successful}");
    out_line!("INLINE_COMMENTS: {inline}");
    out_line!("SUPPRESSED_COMMENTS: {suppressed_count}");
    out_line!(
        "SEVERITY: {}",
        if severity_known { "v2" } else { "unknown" }
    );
    let mut missed_count = 0;
    if let Some(sections) = &sections {
        if severity_known {
            for level in ["High", "Medium", "Low"] {
                let n = sections
                    .items
                    .iter()
                    .filter(|(_, l)| *l == Some(level))
                    .count();
                out_line!("REMAINING_{}: {n}", level.to_uppercase());
            }
        }
        missed_count = sections
            .items
            .iter()
            .filter(|(place, _)| *place == "missed")
            .count();
        out_line!("PREVIOUSLY_MISSED: {missed_count}");
    }
    if suppressed_count != "0" {
        out_line!("--- suppressed ---");
        out_line!("{}", trim_trailing_newlines(&suppressed.join("\n")));
    }
    if missed_count > 0 {
        out_line!("--- previously missed ---");
        let missed = sections
            .as_ref()
            .map_or(String::new(), |s| s.missed.join("\n"));
        out_line!("{}", trim_trailing_newlines(&missed));
    }
    Ok(out)
}

/// 本文（`jq -r .body` と同じく、null は "null"）。行の終わりの改行は除く（sh のコマンド置換と同じ）
fn body_of(review: &Value) -> String {
    trim_trailing_newlines(&text(&review["body"])).to_string()
}

fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn first_line(body: &str) -> &str {
    body.split('\n').next().unwrap_or("")
}

fn trim_trailing_newlines(s: &str) -> &str {
    s.trim_end_matches('\n')
}

/// "Suppressed comments (N)" の行から、次の `</details>` か Files reviewed の行の手前まで。見出しは `<summary>` に
/// 包まれる形もありうるので、行頭に固定しない
fn suppressed_section<'a>(lines: &[&'a str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut on = false;
    for line in lines {
        if SUPPRESSED.is_match(line) {
            on = true;
        }
        if on && (line.contains("</details>") || FILES_REVIEWED.is_match(line)) {
            break;
        }
        if on {
            out.push(*line);
        }
    }
    out
}

/// v2 の本文（2026-09-19 から）の節。指摘ごとに重大度のアイコン（alt="High severity" など）を持ち、節は見出しの行
/// （`<summary><strong>…`）で分かれる。"N open findings"（2026-09 の本文は "Open (N)"）は開いている指摘（その回の
/// 新しい指摘と、前の周から残る指摘。"- " で始まる行）、"Previously missed" は前の周で見落とした、変わっていない
/// コードへの指摘（入れ子の `<summary>` の行。本文にだけあってスレッドにならない）。ほかの節（解決した指摘、
/// "What changed in this PR" の変更の一覧など）は数えない。節は外側の `</details>` で閉じる（Previously missed の中の
/// 入れ子の `<details>` は深さで数える）。節の切り替えを見出しの行だけで行うのは、指摘の題名に同じ語が入りうるため
struct Sections<'a> {
    /// (場所, 重大度)。重大度が None のものは、形が変わって High を数え落としうる印（知らない見出しの節、開いている
    /// 指摘の節の「- 」で始まらない行、Previously missed の節の `<summary>` でない行のアイコン、知っている節の外の
    /// アイコン（要約の **Findings:** の行は除く）、閉じないまま終わる本文、アイコンの無い指摘）で、件数には数えず、
    /// 重大度を分からない（unknown）とする
    items: Vec<(&'static str, Option<&'static str>)>,
    /// Previously missed の節の行
    missed: Vec<&'a str>,
}

#[derive(PartialEq)]
enum Sec {
    None,
    Open,
    Missed,
    Known,
}

impl<'a> Sections<'a> {
    fn read(lines: &[&'a str]) -> Sections<'a> {
        let mut items = Vec::new();
        let mut missed = Vec::new();
        let mut sec = Sec::None;
        let mut depth = 0;
        for &line in lines {
            if line.contains("<summary><strong>") {
                let title = HEADING.replace(&line.to_lowercase(), "").into_owned();
                sec = if title.starts_with("open") {
                    Sec::Open
                } else if title.starts_with("previously missed") {
                    Sec::Missed
                } else if title.starts_with("resolved since last review")
                    || title.starts_with("what changed in this pr")
                {
                    Sec::Known
                } else {
                    items.push(("heading", None));
                    Sec::Known
                };
                depth = 1;
                continue;
            }
            let is_item = ITEM.is_match(line);
            if sec == Sec::Open
                && line.chars().any(|c| !c.is_whitespace())
                && !is_item
                && !line.contains("</details>")
            {
                items.push(("open", None));
            }
            if (sec == Sec::Open && is_item) || (sec == Sec::Missed && line.contains("<summary>")) {
                let place = if sec == Sec::Open { "open" } else { "missed" };
                // 1 行に複数のアイコンがあれば High を優先する
                let level = ["High", "Medium", "Low"]
                    .into_iter()
                    .find(|level| line.contains(&format!("alt=\"{level} severity\"")));
                items.push((place, level));
            }
            if sec == Sec::Missed && !line.contains("<summary>") && SEVERITY_ICON.is_match(line) {
                items.push(("missed-line", None));
            }
            if sec == Sec::None && SEVERITY_ICON.is_match(line) && !line.contains("**Findings:**") {
                items.push(("outside", None));
            }
            if sec == Sec::Missed {
                missed.push(line);
            }
            if line.contains("<details") && sec != Sec::None {
                depth += 1;
            }
            if line.contains("</details>") && sec != Sec::None {
                depth -= 1;
                if depth == 0 {
                    sec = Sec::None;
                }
            }
        }
        // 節が閉じないまま終わる本文（切れた本文）は、残りの指摘を数え落としうる
        if sec != Sec::None {
            items.push(("eof", None));
        }
        Sections { items, missed }
    }
}
