//! ADR の検査。ADR のパスと中身の組を受け取り、違反（error）と注意（warning）をすべて返す。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::parse::{FrontMatter, Value, adr_refs, markdown_lines, parse_front_matter};
use crate::{
    ADR_DIR, AMENDABLE_KEYS, KEYS, NOTES_SECTION, RECOMMENDED_SECTIONS, REQUIRED_SECTIONS,
    STATUSES, Section, adr_number, is_adr_path,
};

/// 違反か注意か。違反があればコミットを止める。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
}

/// 検査で見つかったもの。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    /// リポジトリの直下からのパス（`/` 区切り）。
    pub path: String,
    /// 行番号（1 から）。ファイル全体についてなら `None`。
    pub line: Option<usize>,
    pub severity: Severity,
    pub message: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let severity = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        match self.line {
            Some(line) => write!(f, "{}:{line}: {severity}: {}", self.path, self.message),
            None => write!(f, "{}: {severity}: {}", self.path, self.message),
        }
    }
}

/// `adrs`（docs/adr の直下の .md のパスと中身）の形と、ADR どうしの参照を確かめる。`issues` は docs/issues の
/// issue の番号で、`None`（docs/issues の無いリポジトリ）なら `issues` の要素は形だけを見る。`scanned` は置き換え
/// られた ADR への参照を探すファイル（docs/adr の中のものは見ない）。
pub fn check(
    adrs: &BTreeMap<String, String>,
    issues: Option<&BTreeSet<u32>>,
    scanned: &BTreeMap<String, String>,
) -> Vec<Finding> {
    let mut out = Found::default();
    let docs = parse_adrs(adrs, &mut out);
    let existing: BTreeSet<u32> = docs.iter().map(|d| d.number).collect();
    check_unique(&docs, &mut out);
    for doc in &docs {
        check_front(doc, &mut out);
        check_heading(doc, &mut out);
        check_sections(doc, &mut out);
        check_issues(doc, issues, &mut out);
    }
    check_supersession(&docs, &existing, &mut out);
    check_mentions(&docs, scanned, &mut out);
    let mut found = out.0;
    found.sort();
    found
}

/// 置き換えの関係が効く status（決定が一度は効力を持った）。決める前（proposed、deferred）と、効力を持たずに
/// 終わった（rejected、withdrawn）ADR の `supersedes` は、古い ADR をまだ置き換えていない。
const TOOK_EFFECT: [&str; 3] = ["accepted", "superseded", "deprecated"];

/// 集めている違反と注意。
#[derive(Default)]
struct Found(Vec<Finding>);

impl Found {
    fn push(
        &mut self,
        severity: Severity,
        path: &str,
        line: Option<usize>,
        message: impl ToString,
    ) {
        self.0.push(Finding {
            path: path.to_string(),
            line,
            severity,
            message: message.to_string(),
        });
    }

    fn error(&mut self, path: &str, line: Option<usize>, message: impl ToString) {
        self.push(Severity::Error, path, line, message);
    }

    fn warning(&mut self, path: &str, line: Option<usize>, message: impl ToString) {
        self.push(Severity::Warning, path, line, message);
    }
}

/// 1 件の ADR。
struct Adr<'a> {
    path: &'a str,
    number: u32,
    front: FrontMatter,
    /// コードブロックの外の行（前付けを含む）。コードブロックが閉じていなければ `None` で、行を見る検査を飛ばす。
    lines: Option<Vec<(usize, &'a str)>>,
}

impl Adr<'_> {
    /// 1 つの文字列の `status`。ないか一覧なら空。
    fn status(&self) -> &str {
        match self.front.get("status") {
            Some(Value::Scalar(s)) => s,
            _ => "",
        }
    }

    fn items(&self, key: &str) -> Vec<&str> {
        self.front.get(key).map(Value::items).unwrap_or_default()
    }

    /// `key` の要素のうち、`ADR-NNNN` の形のものの番号。
    fn adr_items(&self, key: &str) -> BTreeSet<u32> {
        self.items(key).into_iter().filter_map(single_adr).collect()
    }
}

/// docs/adr の直下の、番号で始まる .md（[`is_adr_path`]）を読む。名前と前付けが読めないものは違反にして除く。
fn parse_adrs<'a>(adrs: &'a BTreeMap<String, String>, out: &mut Found) -> Vec<Adr<'a>> {
    let mut docs = Vec::new();
    for (path, text) in adrs {
        let Some(name) = path
            .strip_prefix(ADR_DIR)
            .and_then(|rest| rest.strip_prefix('/'))
        else {
            continue;
        };
        if !is_adr_path(path) {
            continue;
        }
        let Some(number) = adr_number(name) else {
            out.error(path, None, "file name is not NNNN-kebab-case.md");
            continue;
        };
        let front = match parse_front_matter(text) {
            Ok(front) => front,
            Err(e) => {
                out.error(path, None, e);
                continue;
            }
        };
        let lines = match markdown_lines(text) {
            Ok(lines) => Some(lines),
            Err(e) => {
                out.error(path, None, e);
                None
            }
        };
        docs.push(Adr {
            path,
            number,
            front,
            lines,
        });
    }
    docs
}

/// 番号の重複。
fn check_unique(docs: &[Adr], out: &mut Found) {
    let mut by_number: BTreeMap<u32, Vec<&str>> = BTreeMap::new();
    for doc in docs {
        by_number.entry(doc.number).or_default().push(doc.path);
    }
    for (number, paths) in by_number {
        if let [first, rest @ ..] = paths.as_slice()
            && !rest.is_empty()
        {
            out.error(
                first,
                None,
                format!("number {number:04} is also used by {}", rest.join(", ")),
            );
        }
    }
}

/// 前付けの値の表示（違反の文に入れる）。
fn show(value: &Value) -> String {
    match value {
        Value::Empty => "empty".to_string(),
        Value::Scalar(s) => format!("{s:?}"),
        Value::List(items) => format!("{items:?}"),
    }
}

/// 前付けのキー、`status` の値、日付。
fn check_front(doc: &Adr, out: &mut Found) {
    for key in KEYS {
        if doc.front.get(key).is_none() {
            out.error(doc.path, None, format!("front matter has no `{key}`"));
        }
    }
    for key in doc.front.keys() {
        if !KEYS.contains(&key) {
            out.error(
                doc.path,
                None,
                format!("unexpected front matter key `{key}`"),
            );
        }
    }
    match doc.front.get("status") {
        Some(Value::Scalar(s)) if STATUSES.contains(&s.as_str()) => {}
        Some(value) => out.error(
            doc.path,
            None,
            format!(
                "`status` must be one of {}, not {}",
                STATUSES.join(", "),
                show(value)
            ),
        ),
        None => {}
    }
    match doc.front.get("date") {
        Some(Value::Scalar(s)) if is_date(s) => {}
        Some(value) => out.error(
            doc.path,
            None,
            format!("`date` is not YYYY-MM-DD: {}", show(value)),
        ),
        None => {}
    }
}

/// 暦にある日付の `YYYY-MM-DD` か（月の日数と閏年を見る）。
fn is_date(s: &str) -> bool {
    let bytes = s.as_bytes();
    let digits = |r: std::ops::Range<usize>| bytes[r].iter().all(u8::is_ascii_digit);
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !digits(0..4)
        || !digits(5..7)
        || !digits(8..10)
    {
        return false;
    }
    let (Ok(year), Ok(month), Ok(day)) = (
        s[0..4].parse::<u32>(),
        s[5..7].parse::<u32>(),
        s[8..10].parse::<u32>(),
    ) else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

/// 最初の見出しが `# ADR-NNNN: 題名` で、番号がファイルの番号と同じか。
fn check_heading(doc: &Adr, out: &mut Found) {
    let Some(lines) = &doc.lines else {
        return;
    };
    let prefix = format!("# ADR-{:04}: ", doc.number);
    let heading = lines.iter().find(|(_, l)| l.starts_with("# "));
    let ok = heading.is_some_and(|(_, l)| {
        l.strip_prefix(&prefix)
            .is_some_and(|t| !t.trim().is_empty())
    });
    if !ok {
        out.error(
            doc.path,
            heading.map(|(n, _)| *n),
            format!("first heading must be `{prefix}<title>`"),
        );
    }
}

/// 2 段目以下の見出しの文字列。
fn subheadings<'a>(lines: &[(usize, &'a str)]) -> BTreeSet<&'a str> {
    lines
        .iter()
        .filter_map(|(_, line)| {
            let rest = line.strip_prefix("##")?.trim_start_matches('#');
            rest.strip_prefix(' ').map(str::trim)
        })
        .collect()
}

/// 必須の節（無ければ違反）と推奨の節（無ければ注意）。日本語の見出しか、MADR の英語の原文で書く。
fn check_sections(doc: &Adr, out: &mut Found) {
    let Some(lines) = &doc.lines else {
        return;
    };
    let headings = subheadings(lines);
    let missing = |s: &Section| !headings.contains(s.ja) && !headings.contains(s.en);
    for section in REQUIRED_SECTIONS.iter().filter(|s| missing(s)) {
        out.error(
            doc.path,
            None,
            format!("no section `{}` (or `{}`)", section.ja, section.en),
        );
    }
    for section in RECOMMENDED_SECTIONS.iter().filter(|s| missing(s)) {
        out.warning(
            doc.path,
            None,
            format!(
                "no section `{}` (or `{}`) (recommended)",
                section.ja, section.en
            ),
        );
    }
}

/// `ADR-NNNN` のちょうど 1 つの参照なら、その番号。
fn single_adr(item: &str) -> Option<u32> {
    match adr_refs(item).as_slice() {
        [n] if item == format!("ADR-{n:04}") => Some(*n),
        _ => None,
    }
}

/// `issues` の要素が `#<番号>` か。docs/issues のあるリポジトリでは、その番号の issue があるか。
fn check_issues(doc: &Adr, issues: Option<&BTreeSet<u32>>, out: &mut Found) {
    for item in doc.items("issues") {
        let number = item
            .strip_prefix('#')
            .filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|n| n.parse::<u32>().ok());
        match (number, issues) {
            (None, _) => out.error(
                doc.path,
                None,
                format!("`issues` item {item:?} is not #<number>"),
            ),
            (Some(n), Some(known)) if !known.contains(&n) => out.error(
                doc.path,
                None,
                format!("`issues` names {item}, which does not exist in docs/issues"),
            ),
            _ => {}
        }
    }
}

/// `requires`、`supersedes`、`superseded-by` の参照先と、置き換えの状態と両方向の一致。
fn check_supersession(docs: &[Adr], existing: &BTreeSet<u32>, out: &mut Found) {
    for doc in docs {
        for key in ["requires", "supersedes", "superseded-by"] {
            for item in doc.items(key) {
                match single_adr(item) {
                    Some(n) if existing.contains(&n) => {}
                    Some(_) => out.error(
                        doc.path,
                        None,
                        format!("`{key}` names {item}, which does not exist"),
                    ),
                    None => out.error(
                        doc.path,
                        None,
                        format!("`{key}` item {item:?} is not ADR-NNNN"),
                    ),
                }
            }
        }
        let superseded = doc.status() == "superseded";
        let by_set = !doc.items("superseded-by").is_empty();
        if superseded && !by_set {
            out.error(
                doc.path,
                None,
                "status is superseded but `superseded-by` is empty",
            );
        }
        if !superseded && by_set {
            out.error(
                doc.path,
                None,
                "`superseded-by` is set but status is not superseded",
            );
        }
    }
    // 番号が重なっていれば、最初の ADR で比べる（重なりは check_unique が示す）
    let mut by_number: BTreeMap<u32, &Adr> = BTreeMap::new();
    for doc in docs {
        by_number.entry(doc.number).or_insert(doc);
    }
    for (&new, doc) in &by_number {
        if !TOOK_EFFECT.contains(&doc.status()) {
            continue;
        }
        for old in doc.adr_items("supersedes") {
            if by_number
                .get(&old)
                .is_some_and(|o| !o.adr_items("superseded-by").contains(&new))
            {
                out.error(
                    doc.path,
                    None,
                    format!(
                        "supersedes ADR-{old:04}, but ADR-{old:04} is not superseded-by ADR-{new:04}"
                    ),
                );
            }
        }
    }
    for (&old, doc) in &by_number {
        for new in doc.adr_items("superseded-by") {
            let Some(successor) = by_number.get(&new) else {
                continue;
            };
            if !successor.adr_items("supersedes").contains(&old) {
                out.error(
                    doc.path,
                    None,
                    format!(
                        "superseded-by ADR-{new:04}, but ADR-{new:04} does not supersede ADR-{old:04}"
                    ),
                );
            } else if !TOOK_EFFECT.contains(&successor.status()) {
                // 置き換える側が効力を持つまで、古い ADR は採用中
                out.error(
                    doc.path,
                    None,
                    format!(
                        "superseded-by ADR-{new:04}, but ADR-{new:04} is {}",
                        successor.status()
                    ),
                );
            }
        }
    }
}

/// 置き換えられた ADR への参照（docs/adr の外）。今の決定を指すよう、置き換えた ADR の番号を示す。
fn check_mentions(docs: &[Adr], scanned: &BTreeMap<String, String>, out: &mut Found) {
    let successors: BTreeMap<u32, Vec<String>> = docs
        .iter()
        .filter(|d| d.status() == "superseded")
        .map(|d| {
            let by = d.adr_items("superseded-by");
            (d.number, by.iter().map(|n| format!("ADR-{n:04}")).collect())
        })
        .filter(|(_, by): &(u32, Vec<String>)| !by.is_empty())
        .collect();
    if successors.is_empty() {
        return;
    }
    for (path, text) in scanned {
        if path.starts_with(&format!("{ADR_DIR}/")) {
            continue;
        }
        // Markdown のコードブロックは例なので見ない。閉じていないコードブロックの扱いはこの検査の外
        // （ADR でないファイルの形は問わない）なので、そのときはすべての行を見る
        let all = || text.lines().enumerate().map(|(i, l)| (i + 1, l)).collect();
        let lines: Vec<(usize, &str)> = if path.ends_with(".md") {
            markdown_lines(text).unwrap_or_else(|_| all())
        } else {
            all()
        };
        for (number, line) in lines {
            for n in adr_refs(line) {
                if let Some(by) = successors.get(&n) {
                    out.warning(
                        path,
                        Some(number),
                        format!("ADR-{n:04} is superseded by {}", by.join(", ")),
                    );
                }
            }
        }
    }
}

/// 前付けと補足の節を除いた本文が変わったか（確定した ADR に許す変更を除いて比べる）。
pub fn body_changed(old: &str, new: &str) -> bool {
    body(old) != body(new)
}

/// 確定した後も直してよい前付けのキー（[`AMENDABLE_KEYS`]）と、補足の節（次の 1〜2 段目の見出しまで）を除いた行。末尾の空行は除く（補足の節を足す前の空行は本文の
/// 変更ではない）。見出しはコードブロックの外で探す。コードブロックが閉じていなければ、全文で比べる（その違反は
/// check が示す）。
fn body(text: &str) -> Vec<&str> {
    let Ok(outside) = markdown_lines(text) else {
        return text.lines().collect();
    };
    let headings: BTreeSet<usize> = outside
        .iter()
        .filter(|(_, l)| l.starts_with("# ") || l.starts_with("## "))
        .map(|(n, _)| *n)
        .collect();
    let mut lines = text.lines().enumerate().map(|(i, l)| (i + 1, l)).peekable();
    let mut front = Vec::new();
    if lines.peek().map(|(_, l)| *l) == Some("---") {
        lines.next();
        for (_, line) in lines.by_ref() {
            if line == "---" {
                break;
            }
            let (key, value) = line.split_once(':').unwrap_or((line, ""));
            // 空のキーはキーが無いのと同じ（前付けの形を揃えるだけの変更を本文の変更にしない）
            let empty = matches!(value.trim(), "" | "[]");
            if !empty && !AMENDABLE_KEYS.contains(&key.trim()) {
                front.push(line);
            }
        }
    }
    let mut in_notes = false;
    let mut kept: Vec<&str> = lines
        .filter(|(n, line)| {
            if headings.contains(n) {
                let title = line.trim_start_matches('#').trim();
                in_notes = line.starts_with("## ")
                    && (title == NOTES_SECTION.ja || title == NOTES_SECTION.en);
            }
            !in_notes
        })
        .map(|(_, line)| line)
        .collect();
    kept.splice(0..0, front);
    while kept.last().is_some_and(|l| l.trim().is_empty()) {
        kept.pop();
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Adr<'a> {
        n: u32,
        status: &'a str,
        requires: &'a str,
        supersedes: &'a str,
        superseded_by: &'a str,
        issues: &'a str,
    }

    impl Default for Adr<'_> {
        fn default() -> Self {
            Adr {
                n: 1,
                status: "accepted",
                requires: "[]",
                supersedes: "[]",
                superseded_by: "",
                issues: "[]",
            }
        }
    }

    impl Adr<'_> {
        fn path(&self) -> String {
            format!("docs/adr/{:04}-decide-{}.md", self.n, self.n)
        }

        /// 見出しは 10 行目
        fn text(&self) -> String {
            format!(
                "---\n\
                 status: {}\n\
                 date: 2026-10-11\n\
                 requires: {}\n\
                 supersedes: {}\n\
                 superseded-by:{}\n\
                 issues: {}\n\
                 ---\n\
                 \n\
                 # ADR-{:04}: 決めたこと\n\
                 \n\
                 ## 背景\n\
                 \n\
                 ## 検討した案\n\
                 \n\
                 ## 決定と理由\n\
                 \n\
                 ### 帰結\n\
                 \n\
                 ### 確認\n",
                self.status,
                self.requires,
                self.supersedes,
                if self.superseded_by.is_empty() {
                    String::new()
                } else {
                    format!(" {}", self.superseded_by)
                },
                self.issues,
                self.n,
            )
        }
    }

    fn files(items: &[(String, String)]) -> BTreeMap<String, String> {
        items.iter().cloned().collect()
    }

    fn adrs(list: &[Adr]) -> BTreeMap<String, String> {
        list.iter().map(|a| (a.path(), a.text())).collect()
    }

    /// 見つかったものを表示の形（`<path>[:<line>]: <severity>: <message>`）にする
    fn run(adrs: &BTreeMap<String, String>) -> Vec<String> {
        shown(check(adrs, None, &BTreeMap::new()))
    }

    fn shown(found: Vec<Finding>) -> Vec<String> {
        found.iter().map(ToString::to_string).collect()
    }

    fn one(n: u32, edit: impl Fn(String) -> String) -> BTreeMap<String, String> {
        let adr = Adr {
            n,
            ..Default::default()
        };
        files(&[(adr.path(), edit(adr.text()))])
    }

    #[test]
    fn findings_show_path_line_and_severity() {
        let f = Finding {
            path: "docs/adr/0001-a.md".into(),
            line: Some(3),
            severity: Severity::Error,
            message: "m".into(),
        };
        assert_eq!(f.to_string(), "docs/adr/0001-a.md:3: error: m");
        let f = Finding {
            line: None,
            severity: Severity::Warning,
            ..f
        };
        assert_eq!(f.to_string(), "docs/adr/0001-a.md: warning: m");
    }

    #[test]
    fn well_formed_adrs_pass() {
        let set = adrs(&[
            Adr {
                n: 1,
                status: "superseded",
                superseded_by: "ADR-0002",
                ..Default::default()
            },
            Adr {
                n: 2,
                supersedes: "[ADR-0001]",
                issues: "[\"#269\"]",
                ..Default::default()
            },
            Adr {
                n: 3,
                status: "proposed",
                requires: "[ADR-0002]",
                ..Default::default()
            },
        ]);
        assert_eq!(run(&set), Vec::<String>::new());
    }

    #[test]
    fn readme_and_other_files_are_not_adrs() {
        let mut set = adrs(&[Adr::default()]);
        set.insert("docs/adr/README.md".into(), "# ADR\n".into());
        assert_eq!(run(&set), Vec::<String>::new());
    }

    #[test]
    fn unnumbered_files_are_not_adrs() {
        // claude-hooks（is_adr_path）と同じく、番号で始まらないファイルは ADR として数えない
        let mut set = adrs(&[Adr::default()]);
        set.insert("docs/adr/template.md".into(), "# template\n".into());
        assert_eq!(run(&set), Vec::<String>::new());
    }

    #[test]
    fn file_names_are_numbered_kebab_case() {
        let set = files(&[("docs/adr/0001-Use_F.md".into(), Adr::default().text())]);
        assert_eq!(
            run(&set),
            ["docs/adr/0001-Use_F.md: error: file name is not NNNN-kebab-case.md"]
        );
    }

    #[test]
    fn numbers_are_unique() {
        let a = Adr::default();
        let set = files(&[
            ("docs/adr/0001-a.md".into(), a.text()),
            ("docs/adr/0001-b.md".into(), a.text()),
        ]);
        assert_eq!(
            run(&set),
            ["docs/adr/0001-a.md: error: number 0001 is also used by docs/adr/0001-b.md"]
        );
    }

    #[test]
    fn front_matter_has_exactly_the_keys() {
        let set = one(1, |t| {
            t.replace("requires: []\n", "decision-makers: [me]\n")
        });
        assert_eq!(
            run(&set),
            [
                "docs/adr/0001-decide-1.md: error: front matter has no `requires`",
                "docs/adr/0001-decide-1.md: error: unexpected front matter key `decision-makers`",
            ]
        );
    }

    #[test]
    fn front_matter_must_parse() {
        let set = one(1, |t| t.replacen("---\n", "", 1));
        assert_eq!(
            run(&set),
            ["docs/adr/0001-decide-1.md: error: the file does not start with a `---` front matter"]
        );
    }

    #[test]
    fn status_and_date_take_known_values() {
        let set = one(1, |t| {
            t.replace("status: accepted", "status: approved")
                .replace("2026-10-11", "2026-02-30")
        });
        assert_eq!(
            run(&set),
            [
                "docs/adr/0001-decide-1.md: error: `date` is not YYYY-MM-DD: \"2026-02-30\"",
                "docs/adr/0001-decide-1.md: error: `status` must be one of proposed, accepted, rejected, withdrawn, deferred, superseded, deprecated, not \"approved\"",
            ]
        );
    }

    #[test]
    fn title_heading_carries_the_file_number() {
        let set = one(2, |t| t.replace("# ADR-0002: ", "# ADR-0003: "));
        assert_eq!(
            run(&set),
            ["docs/adr/0002-decide-2.md:10: error: first heading must be `# ADR-0002: <title>`"]
        );
    }

    #[test]
    fn required_sections_are_errors_and_recommended_are_warnings() {
        let set = one(1, |t| {
            t.replace("## 検討した案\n", "").replace("### 確認\n", "")
        });
        assert_eq!(
            run(&set),
            [
                "docs/adr/0001-decide-1.md: error: no section `検討した案` (or `Considered Options`)",
                "docs/adr/0001-decide-1.md: warning: no section `確認` (or `Confirmation`) (recommended)",
            ]
        );
    }

    #[test]
    fn sections_may_use_madr_english_headings() {
        let set = one(1, |t| {
            t.replace("## 背景", "## Context and Problem Statement")
                .replace("### 帰結", "### Consequences")
        });
        assert_eq!(run(&set), Vec::<String>::new());
    }

    #[test]
    fn headings_in_code_blocks_do_not_count() {
        let set = one(1, |t| {
            t.replace("## 決定と理由\n", "```markdown\n## 決定と理由\n```\n")
        });
        assert_eq!(
            run(&set),
            ["docs/adr/0001-decide-1.md: error: no section `決定と理由` (or `Decision Outcome`)"]
        );
    }

    #[test]
    fn unclosed_code_blocks_are_errors() {
        let set = one(1, |t| t + "```\n");
        assert_eq!(
            run(&set),
            ["docs/adr/0001-decide-1.md: error: code fence opened on line 21 is not closed"]
        );
    }

    #[test]
    fn superseded_status_and_superseded_by_go_together() {
        let set = adrs(&[
            Adr {
                n: 1,
                status: "superseded",
                ..Default::default()
            },
            Adr {
                n: 2,
                superseded_by: "ADR-0003",
                ..Default::default()
            },
            Adr {
                n: 3,
                supersedes: "[ADR-0002]",
                ..Default::default()
            },
        ]);
        assert_eq!(
            run(&set),
            [
                "docs/adr/0001-decide-1.md: error: status is superseded but `superseded-by` is empty",
                "docs/adr/0002-decide-2.md: error: `superseded-by` is set but status is not superseded",
            ]
        );
    }

    #[test]
    fn decided_supersedes_needs_superseded_by_on_the_old_adr() {
        let set = adrs(&[
            Adr {
                n: 1,
                ..Default::default()
            },
            Adr {
                n: 2,
                supersedes: "[ADR-0001]",
                ..Default::default()
            },
        ]);
        assert_eq!(
            run(&set),
            [
                "docs/adr/0002-decide-2.md: error: supersedes ADR-0001, but ADR-0001 is not superseded-by ADR-0002"
            ]
        );
    }

    #[test]
    fn undecided_supersedes_leaves_the_old_adr_in_effect() {
        // 決める前の ADR が置き換える予定の ADR は、まだ採用中
        for status in ["proposed", "deferred", "rejected", "withdrawn"] {
            let set = adrs(&[
                Adr {
                    n: 1,
                    ..Default::default()
                },
                Adr {
                    n: 2,
                    status,
                    supersedes: "[ADR-0001]",
                    ..Default::default()
                },
            ]);
            assert_eq!(run(&set), Vec::<String>::new(), "{status}");
        }
    }

    #[test]
    fn superseded_by_needs_supersedes_on_the_new_adr() {
        let set = adrs(&[
            Adr {
                n: 1,
                status: "superseded",
                superseded_by: "ADR-0002",
                ..Default::default()
            },
            Adr {
                n: 2,
                status: "proposed",
                ..Default::default()
            },
        ]);
        assert_eq!(
            run(&set),
            [
                "docs/adr/0001-decide-1.md: error: superseded-by ADR-0002, but ADR-0002 does not supersede ADR-0001"
            ]
        );
    }

    #[test]
    fn old_adr_is_superseded_only_after_the_new_one_took_effect() {
        // 置き換える側が決まる前は、古い ADR はまだ採用中
        for status in ["proposed", "deferred", "rejected", "withdrawn"] {
            let set = adrs(&[
                Adr {
                    n: 1,
                    status: "superseded",
                    superseded_by: "ADR-0002",
                    ..Default::default()
                },
                Adr {
                    n: 2,
                    status,
                    supersedes: "[ADR-0001]",
                    ..Default::default()
                },
            ]);
            assert_eq!(
                run(&set),
                [format!(
                    "docs/adr/0001-decide-1.md: error: superseded-by ADR-0002, but ADR-0002 is {status}"
                )],
                "{status}"
            );
        }
    }

    #[test]
    fn adr_references_name_existing_adrs() {
        let set = adrs(&[Adr {
            n: 1,
            requires: "[ADR-0009, \"0002\"]",
            ..Default::default()
        }]);
        assert_eq!(
            run(&set),
            [
                "docs/adr/0001-decide-1.md: error: `requires` item \"0002\" is not ADR-NNNN",
                "docs/adr/0001-decide-1.md: error: `requires` names ADR-0009, which does not exist",
            ]
        );
    }

    #[test]
    fn issues_are_hash_numbers() {
        let set = adrs(&[Adr {
            issues: "[\"#12\", \"12\", \"#x\"]",
            ..Default::default()
        }]);
        assert_eq!(
            run(&set),
            [
                "docs/adr/0001-decide-1.md: error: `issues` item \"#x\" is not #<number>",
                "docs/adr/0001-decide-1.md: error: `issues` item \"12\" is not #<number>",
            ]
        );
    }

    #[test]
    fn issues_name_existing_files_when_the_repository_keeps_docs_issues() {
        let set = adrs(&[Adr {
            issues: "[\"#0058\", \"#0059\"]",
            ..Default::default()
        }]);
        let known = BTreeSet::from([58]);
        assert_eq!(
            shown(check(&set, Some(&known), &BTreeMap::new())),
            [
                "docs/adr/0001-decide-1.md: error: `issues` names #0059, which does not exist in docs/issues"
            ]
        );
    }

    #[test]
    fn mentions_of_superseded_adrs_are_warnings() {
        let set = adrs(&[
            Adr {
                n: 1,
                status: "superseded",
                superseded_by: "ADR-0002",
                ..Default::default()
            },
            Adr {
                n: 2,
                supersedes: "[ADR-0001]",
                ..Default::default()
            },
        ]);
        let scanned = files(&[
            (
                "README.md".into(),
                "ADR-0002 に従う\n```\nADR-0001\n```\n古くは ADR-0001\n".into(),
            ),
            ("src/main.rs".into(), "// ADR-0001 による\n".into()),
            // ADR の中の参照は置き換えの経緯なので見ない
            ("docs/adr/0002-decide-2.md".into(), "ADR-0001\n".into()),
        ]);
        assert_eq!(
            shown(check(&set, None, &scanned)),
            [
                "README.md:5: warning: ADR-0001 is superseded by ADR-0002",
                "src/main.rs:1: warning: ADR-0001 is superseded by ADR-0002",
            ]
        );
    }

    const BODY: &str = "---\nstatus: accepted\ndate: 2026-10-01\n---\n\n# ADR-0001: a\n\n## 背景\n\n文\n\n## 補足\n\n- 前\n";

    #[test]
    fn front_matter_and_notes_changes_keep_the_body() {
        let new = BODY
            .replace("status: accepted", "status: superseded")
            .replace("2026-10-01", "2026-10-11")
            + "- 2026-10-11: 追記\n";
        assert!(!body_changed(BODY, &new));
    }

    #[test]
    fn other_front_matter_changes_change_the_body() {
        // 確定した後に直してよい前付けは status、superseded-by、date だけ
        let old = BODY.replace("date: 2026-10-01\n", "date: 2026-10-01\nsupersedes: []\n");
        assert!(body_changed(
            &old,
            &old.replace("supersedes: []", "supersedes: [ADR-0003]")
        ));
    }

    #[test]
    fn adding_empty_keys_keeps_the_body() {
        // 空のキーはキーが無いのと同じ（前付けの形を揃えるだけの変更）
        let new = BODY.replace(
            "date: 2026-10-01\n",
            "date: 2026-10-01\nrequires: []\nsuperseded-by:\nissues: []\n",
        );
        assert!(!body_changed(BODY, &new));
    }

    #[test]
    fn english_notes_section_is_also_open() {
        let old = BODY.replace("## 補足", "## More Information");
        assert!(!body_changed(
            &old,
            &(old.clone() + "- 2026-10-11: added\n")
        ));
    }

    #[test]
    fn hashes_in_code_blocks_do_not_end_or_start_notes() {
        let new =
            BODY.to_string() + "```bash\n# comment\n## not a heading\n```\n- 2026-10-11: 追記\n";
        assert!(!body_changed(BODY, &new));
        let old = BODY.replace("文\n", "文\n\n```markdown\n## 補足\n```\n\nあと\n");
        assert!(body_changed(&old, &old.replace("あと", "別")));
    }

    #[test]
    fn other_changes_change_the_body() {
        assert!(body_changed(BODY, &BODY.replace("文", "別の文")));
        // 補足の後に別の節があれば、その節は本文
        let old = BODY.to_string() + "\n## 付録\n\nx\n";
        assert!(body_changed(&old, &old.replace("\nx\n", "\ny\n")));
    }
}
