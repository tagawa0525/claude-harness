//! 設計の記録（ADR）の形。フロントマターのキーと status、必須と推奨の節、ファイル名と番号、テンプレートを持つ（ADR-0009）。
//! ADR と docs/issues の issue の番号の予約（タグの名前）と振り直し（参照の付け替え）も持つ（ADR-0018）。
//!
//! 形の値を持つのはこのクレートだけにする。スキル `adr` と `.claude/scripts/branch-topics.sh` は CLI を呼び、
//! claude-hooks はライブラリとしてパスの判定を使う。

pub mod check;
pub mod parse;

use std::path::Path;

/// ADR を置くディレクトリ（リポジトリの直下から）。
pub const ADR_DIR: &str = "docs/adr";

/// issue を置くディレクトリ（リポジトリの直下から）。
pub const ISSUE_DIR: &str = "docs/issues";

/// 番号を振る記録の種類。どちらも `<dir>/NNNN-<slug>.md` に置き、番号を再利用しない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Adr,
    Issue,
}

impl Kind {
    pub const ALL: [Kind; 2] = [Kind::Adr, Kind::Issue];

    /// CLI の `--kind` の値から。
    pub fn parse(name: &str) -> Option<Kind> {
        match name {
            "adr" => Some(Kind::Adr),
            "issue" => Some(Kind::Issue),
            _ => None,
        }
    }

    /// CLI の `--kind` の値。
    pub fn name(self) -> &'static str {
        match self {
            Kind::Adr => "adr",
            Kind::Issue => "issue",
        }
    }

    /// 置くディレクトリ。
    pub fn dir(self) -> &'static str {
        match self {
            Kind::Adr => ADR_DIR,
            Kind::Issue => ISSUE_DIR,
        }
    }

    /// 本文での参照の、番号の前の部分（`ADR-0018`、`#0018`）。
    pub fn ref_prefix(self) -> &'static str {
        match self {
            Kind::Adr => "ADR-",
            Kind::Issue => "#",
        }
    }

    /// 本文での参照。
    pub fn reference(self, number: u32) -> String {
        format!("{}{number:04}", self.ref_prefix())
    }

    /// 番号を予約するタグの名前（`adr-0018`、`issue-0018`）。
    pub fn tag(self, number: u32) -> String {
        format!("{}-{number:04}", self.name())
    }

    /// 記録のファイルのパス（`<dir>/NNNN-<slug>.md`）。
    pub fn path(self, number: u32, slug: &str) -> String {
        format!("{}/{number:04}-{slug}.md", self.dir())
    }
}

/// 予約のタグの名前（`adr-NNNN`、`issue-NNNN`。4 桁）から、種類と番号を読む。
pub fn parse_tag(name: &str) -> Option<(Kind, u32)> {
    Kind::ALL.into_iter().find_map(|kind| {
        let digits = name.strip_prefix(kind.name())?.strip_prefix('-')?;
        if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        Some((kind, digits.parse().ok()?))
    })
}

/// フロントマターのキー（書く順）。
pub const KEYS: [&str; 6] = [
    "status",
    "date",
    "requires",
    "supersedes",
    "superseded-by",
    "issues",
];

/// 書いてもよいフロントマターのキー。決定の一部の置き換えを両方向に結ぶ: `amends` は
/// この ADR が一部を置き換えた ADR、`amended-by` はこの ADR の一部を置き換えた ADR（ADR-0051）。一部の置き換えは
/// まれなので、すべての ADR には求めない。
pub const OPTIONAL_KEYS: [&str; 2] = ["amends", "amended-by"];

/// `status` の値。
pub const STATUSES: [&str; 7] = [
    "proposed",
    "accepted",
    "rejected",
    "withdrawn",
    "deferred",
    "superseded",
    "deprecated",
];

/// 決める前の status。この間は本文を自由に直してよい。
pub const OPEN_STATUSES: [&str; 2] = ["proposed", "deferred"];

/// 確定した後も直してよいフロントマターのキー。
/// `amended-by` は新しい ADR が決まったときに古い ADR に書く。`amends` は、確定した ADR の本文に書いた一部の置き換えを
/// フロントマターにも書くために許す。どちらも相手の ADR との一致は check が確かめる（ADR-0051）。
pub const AMENDABLE_KEYS: [&str; 5] = ["status", "superseded-by", "date", "amends", "amended-by"];

/// 節の見出し（日本語と、MADR の英語の原文）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    pub ja: &'static str,
    pub en: &'static str,
}

/// 必須の節。
pub const REQUIRED_SECTIONS: [Section; 3] = [
    Section {
        ja: "背景",
        en: "Context and Problem Statement",
    },
    Section {
        ja: "検討した案",
        en: "Considered Options",
    },
    Section {
        ja: "決定と理由",
        en: "Decision Outcome",
    },
];

/// 推奨の節。
pub const RECOMMENDED_SECTIONS: [Section; 2] = [
    Section {
        ja: "帰結",
        en: "Consequences",
    },
    Section {
        ja: "確認",
        en: "Confirmation",
    },
];

/// `status` の ADR に求める推奨の節。採らなかった決定（rejected）には、確かめるものも帰結も無く、番号だけを残した
/// ADR は確定後に書き足せないので、求めない（ADR-0049）。
pub fn recommended_sections(status: &str) -> &'static [Section] {
    if status == "rejected" {
        &[]
    } else {
        &RECOMMENDED_SECTIONS
    }
}

/// ADR のファイルか（docs/adr/ 直下の、4 桁の番号とハイフンで始まる .md）。README やテンプレートは含めない。
/// 名前の残りの形（kebab-case）は問わない。形の崩れは検査（[`check`]）が示す。
pub fn is_adr_path(path: &str) -> bool {
    path.strip_prefix(ADR_DIR)
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(|name| {
            let b = name.as_bytes();
            !name.contains('/')
                && name.ends_with(".md")
                && b.len() > 5
                && b[..4].iter().all(u8::is_ascii_digit)
                && b[4] == b'-'
        })
}

/// 表題（`# ADR-NNNN: ` の後ろ）にできるか: 空白だけでない 1 行。
pub fn is_title(title: &str) -> bool {
    !title.trim().is_empty() && !title.contains(['\n', '\r'])
}

/// `NNNN-kebab-case.md` の形のファイル名から番号を取り出す。形が違えば `None`。
pub fn record_number(name: &str) -> Option<u32> {
    let (number, rest) = name.strip_suffix(".md")?.split_once('-')?;
    if number.len() != 4 || !number.bytes().all(|b| b.is_ascii_digit()) || !is_slug(rest) {
        return None;
    }
    number.parse().ok()
}

/// 英小文字と数字の語を `-` でつないだもの（`[a-z0-9]+(-[a-z0-9]+)*`）か。
pub fn is_slug(name: &str) -> bool {
    name.split('-').all(|word| {
        !word.is_empty()
            && word
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    })
}

/// `paths` のうち `<kind の dir>/<数字>-*.md` の番号（4 桁まで。3 桁で振っていた記録も数える。5 桁以上は日付などの
/// 番号でない数字なので数えない）。
pub fn numbers_in<'a>(
    kind: Kind,
    paths: impl IntoIterator<Item = &'a str>,
) -> impl Iterator<Item = u32> {
    paths.into_iter().filter_map(move |path| {
        let name = path.strip_prefix(kind.dir())?.strip_prefix('/')?;
        let (number, _) = name.strip_suffix(".md")?.split_once('-')?;
        if name.contains('/') || !(1..=4).contains(&number.len()) {
            return None;
        }
        number.parse::<u32>().ok()
    })
}

/// `path` が `kind` の記録（`<dir>/<数字>-<slug>.md`。数字は [`numbers_in`] と同じく 4 桁まで）なら、番号と slug。
pub fn record_name(kind: Kind, path: &str) -> Option<(u32, &str)> {
    let number = numbers_in(kind, [path]).next()?;
    let name = path.rsplit('/').next()?;
    let slug = name.strip_suffix(".md")?.split_once('-')?.1;
    (!slug.is_empty()).then_some((number, slug))
}

/// 次の番号。`paths` の `kind` の記録の番号（[`numbers_in`]）の最大 + 1。無ければ 1。
pub fn next_number<'a>(kind: Kind, paths: impl IntoIterator<Item = &'a str>) -> u32 {
    numbers_in(kind, paths).max().map_or(1, |n| n + 1)
}

/// 新しい issue のテンプレート。フロントマターと節の形は各リポジトリが決めるので、表題の行だけを書く（ADR-0018）。
pub fn issue_template(title: &str) -> String {
    format!("# {title}\n")
}

/// 新しい ADR のテンプレート。status は proposed、フロントマターのキーは空、必須と推奨の見出しを持つ。
pub fn template(number: u32, title: &str, date: &str) -> String {
    let mut text = String::from("---\n");
    for key in KEYS {
        let value = match key {
            "status" => " proposed".to_string(),
            "date" => format!(" {date}"),
            // 置き換えられた ADR は 1 つの番号を書くので、一覧にしない
            "superseded-by" => String::new(),
            _ => " []".to_string(),
        };
        text.push_str(&format!("{key}:{value}\n"));
    }
    text.push_str(&format!("---\n\n# ADR-{number:04}: {title}\n"));
    // 必須の節は MADR と同じ 2 段目、推奨の節は決定と理由の下の 3 段目
    for section in REQUIRED_SECTIONS {
        text.push_str(&format!("\n## {}\n", section.ja));
    }
    for section in RECOMMENDED_SECTIONS {
        text.push_str(&format!("\n### {}\n", section.ja));
    }
    text
}

/// `line` のうち、`kind` の記録 `old`（slug は `slug`）を指す参照の番号を `new` に付け替える。`doc_dir` は `line` の
/// 文書のディレクトリ（リポジトリの直下から）。参照は、本文での参照（[`Kind::reference`]。[`parse::numbered_refs`] と
/// 同じ読み方）と、ファイル名の `NNNN-<slug>.md`。ファイル名は、前のディレクトリの部分を文書のディレクトリか
/// リポジトリの直下から解いて、その種類のディレクトリを指すものだけ（`./`、`../` を含む）。ディレクトリの部分の
/// 無い名前は、文書がほかの種類のディレクトリにあれば、その種類の記録（同じ番号と slug のことがある）とみなして
/// 書き換えない。`.md` の後ろに名前が続くもの（`.mdx`、`.md.orig`、`.md-old`）は別のファイル。
pub fn renumber_refs(
    kind: Kind,
    line: &str,
    old: u32,
    new: u32,
    slug: &str,
    doc_dir: &Path,
) -> String {
    let starts: std::collections::BTreeSet<usize> = parse::numbered_refs(line, kind.ref_prefix())
        .into_iter()
        .filter(|&(_, n)| n == old)
        .map(|(i, _)| i + kind.ref_prefix().len())
        .collect();
    // 番号の始まりから終わりまで。本文の参照は 4 桁、ファイル名は 3 桁で振った記録（[`numbers_in`]）も含む
    let mut spans: std::collections::BTreeMap<usize, usize> =
        starts.into_iter().map(|start| (start, start + 4)).collect();
    let file = format!("-{slug}.md");
    let kind_dir = Path::new(kind.dir());
    let bare_is_kind = !Kind::ALL
        .into_iter()
        .any(|other| other != kind && doc_dir == Path::new(other.dir()));
    for (dash, _) in line.match_indices(&file) {
        let digits = line[..dash].len()
            - line[..dash]
                .trim_end_matches(|c: char| c.is_ascii_digit())
                .len();
        let start = dash - digits;
        let before = &line[..start];
        let prefix = &before[before.trim_end_matches(is_path_char).len()..];
        let of_kind = if prefix.is_empty() {
            bare_is_kind
        } else if let Some(dir) = prefix.strip_suffix('/').filter(|d| !d.starts_with('/')) {
            let dir = Path::new(dir);
            normalize(&doc_dir.join(dir)) == kind_dir || normalize(dir) == kind_dir
        } else {
            // 語の途中（x0001-…）や、/ で終わらない前置き、絶対のパス
            false
        };
        let mut after = line[dash + file.len()..].chars();
        let ends = match after.next() {
            Some(c) if parse::is_word(c) => false,
            // `.md.orig` と `.md-old` は続く名前。文末の `.` や区切りの `-` は名前に含めない
            Some('.' | '-') => !after.next().is_some_and(parse::is_word),
            _ => true,
        };
        if (1..=4).contains(&digits) && line[start..dash].parse() == Ok(old) && of_kind && ends {
            spans.insert(start, dash);
        }
    }
    let mut out = String::with_capacity(line.len());
    let mut at = 0;
    for (start, end) in spans {
        out.push_str(&line[at..start]);
        out.push_str(&format!("{new:04}"));
        at = end;
    }
    out.push_str(&line[at..]);
    out
}

/// ファイル名の参照の前のディレクトリの部分に使う文字
fn is_path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/')
}

/// `.` と `..` を畳んだパス（ファイルは見ない。先頭を越える `..` は残す）
fn normalize(path: &Path) -> std::path::PathBuf {
    use std::path::Component;
    let mut out: Vec<Component> = Vec::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir if matches!(out.last(), Some(Component::Normal(_))) => {
                out.pop();
            }
            _ => out.push(c),
        }
    }
    out.iter().collect()
}

/// フロントマターの最初の `status:` の行の値（後ろの ` # コメント` と引用符は外す）。フロントマターか値が無ければ `None`。
/// フロントマターのほかの行の形は問わない（形を検査していない文書も読むため。形の検査は [`check`] が行う）。
pub fn status(text: &str) -> Option<String> {
    let mut lines = text.lines();
    if lines.next() != Some("---") {
        return None;
    }
    let line = lines
        .take_while(|l| *l != "---")
        .find_map(|l| l.strip_prefix("status:"))?;
    // 空白に続く `#` からはコメント
    let end = line
        .char_indices()
        .find(|&(i, c)| c == '#' && line[..i].ends_with(char::is_whitespace))
        .map_or(line.len(), |(i, _)| i);
    let value = line[..end].trim().trim_matches('"');
    (!value.is_empty()).then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adr_paths_are_numbered_markdown_directly_under_docs_adr() {
        assert!(is_adr_path("docs/adr/0002-use-b.md"));
        assert!(is_adr_path("docs/adr/0002-日本語.md"));
        assert!(!is_adr_path("docs/adr/README.md"));
        assert!(!is_adr_path("docs/adr/template.md"));
        assert!(!is_adr_path("docs/adr/drafts/0003-x.md"));
        assert!(!is_adr_path("docs/adrs/0004-x.md"));
        assert!(!is_adr_path("sub/docs/adr/0005-x.md"));
        assert!(!is_adr_path("docs/adr/0006-x.txt"));
        assert!(!is_adr_path("docs/adr/1-notes.md"));
        assert!(!is_adr_path("docs/adr/2026notes.md"));
        assert!(!is_adr_path("docs/adr/00012-x.md"));
    }

    #[test]
    fn reads_numbers_from_kebab_case_names() {
        assert_eq!(record_number("0013-docs-consistency-check.md"), Some(13));
        assert_eq!(record_number("0005-k-offset-2d.md"), Some(5));
        assert_eq!(record_number("README.md"), None);
        assert_eq!(record_number("013-short.md"), None);
        assert_eq!(record_number("00013-long.md"), None);
        assert_eq!(record_number("0013-name.txt"), None);
        assert_eq!(record_number("0013.md"), None);
        assert_eq!(record_number("0013-.md"), None);
        assert_eq!(record_number("0013-Docs-check.md"), None);
        assert_eq!(record_number("0013-docs_check.md"), None);
        assert_eq!(record_number("0013-docs--check.md"), None);
        assert_eq!(record_number("0013-文書.md"), None);
    }

    #[test]
    fn slugs_are_lowercase_kebab_case() {
        assert!(is_slug("adopt-madr-for-adrs"));
        assert!(is_slug("use-v2"));
        assert!(!is_slug(""));
        assert!(!is_slug("Use_F"));
        assert!(!is_slug("-a"));
        assert!(!is_slug("a-"));
        assert!(!is_slug("a--b"));
    }

    #[test]
    fn next_number_follows_the_largest_of_any_width() {
        assert_eq!(next_number(Kind::Adr, []), 1);
        assert_eq!(
            next_number(
                Kind::Adr,
                ["docs/adr/0001-a.md", "docs/adr/0003-c.md", "README.md"]
            ),
            4
        );
        // 3 桁で振っていた ADR も数える
        assert_eq!(
            next_number(Kind::Adr, ["docs/adr/001-a.md", "docs/adr/007-g.md"]),
            8
        );
        // docs/adr の直下でないもの、番号で始まらないものは数えない
        assert_eq!(
            next_number(
                Kind::Adr,
                [
                    "docs/adr/README.md",
                    "docs/adr/drafts/0009-x.md",
                    "docs/issues/0042-y.md",
                    "docs/adr/0002-b.md",
                ]
            ),
            3
        );
    }

    #[test]
    fn next_number_ignores_numbers_longer_than_four_digits() {
        assert_eq!(
            next_number(
                Kind::Adr,
                ["docs/adr/0002-b.md", "docs/adr/20261010-notes.md"]
            ),
            3
        );
    }

    #[test]
    fn next_issue_number_counts_only_docs_issues() {
        assert_eq!(
            next_number(
                Kind::Issue,
                [
                    "docs/adr/0009-a.md",
                    "docs/issues/0002-b.md",
                    "docs/issues/README.md"
                ]
            ),
            3
        );
    }

    #[test]
    fn reservation_tags_carry_the_kind_and_four_digits() {
        assert_eq!(Kind::Adr.tag(18), "adr-0018");
        assert_eq!(Kind::Issue.tag(7), "issue-0007");
        assert_eq!(parse_tag("adr-0018"), Some((Kind::Adr, 18)));
        assert_eq!(parse_tag("issue-0007"), Some((Kind::Issue, 7)));
        for name in [
            "adr-18",
            "adr-00018",
            "adr-001x",
            "v1.0",
            "adrs-0001",
            "issue0001",
        ] {
            assert_eq!(parse_tag(name), None, "{name}");
        }
    }

    #[test]
    fn renumber_refs_rewrites_only_paths_that_resolve_to_the_record() {
        let adr = Path::new("docs/adr");
        // 文書のディレクトリかリポジトリの直下から解いて、その種類のディレクトリを指すものだけを書き換える
        assert_eq!(
            renumber_refs(
                Kind::Adr,
                "./0001-use-b.md ../issues/0001-use-b.md docs/issues/0001-use-b.md docs/adr/0001-use-b.md",
                1,
                2,
                "use-b",
                adr
            ),
            "./0002-use-b.md ../issues/0001-use-b.md docs/issues/0001-use-b.md docs/adr/0002-use-b.md"
        );
        assert_eq!(
            renumber_refs(
                Kind::Adr,
                "../../adr/0001-use-b.md vendor/x/docs/adr/0001-use-b.md foo-adr/0001-use-b.md",
                1,
                2,
                "use-b",
                Path::new("docs/plans/sub")
            ),
            "../../adr/0002-use-b.md vendor/x/docs/adr/0001-use-b.md foo-adr/0001-use-b.md"
        );
        // .md の後ろが続く名前は別のファイル
        assert_eq!(
            renumber_refs(
                Kind::Adr,
                "0001-use-b.mdx 0001-use-b.md.orig 0001-use-b.md-old 0001-use-b.md。",
                1,
                2,
                "use-b",
                adr
            ),
            "0001-use-b.mdx 0001-use-b.md.orig 0001-use-b.md-old 0002-use-b.md。"
        );
        // 裸のファイル名は、文書がほかの種類のディレクトリにあれば、その種類の記録とみなして書き換えない
        assert_eq!(
            renumber_refs(
                Kind::Adr,
                "[b](0001-use-b.md)",
                1,
                2,
                "use-b",
                Path::new("docs/issues")
            ),
            "[b](0001-use-b.md)"
        );
        assert_eq!(
            renumber_refs(
                Kind::Issue,
                "（0001-fix-b.md）",
                1,
                3,
                "fix-b",
                Path::new("")
            ),
            "（0003-fix-b.md）"
        );
    }

    #[test]
    fn renumber_refs_rewrites_references_and_file_names_of_the_record() {
        assert_eq!(
            renumber_refs(
                Kind::Adr,
                "ADR-0001 と [b](0001-use-b.md)、docs/adr/0001-use-b.md",
                1,
                2,
                "use-b",
                Path::new("docs/adr")
            ),
            "ADR-0002 と [b](0002-use-b.md)、docs/adr/0002-use-b.md"
        );
        // 別の番号、別の slug、語の途中、桁の違うものは変えない
        assert_eq!(
            renumber_refs(
                Kind::Adr,
                "ADR-0011 0001-use-a.md x0001-use-b.md XADR-0001 ADR-00012 ADR-0001x",
                1,
                2,
                "use-b",
                Path::new("docs/adr")
            ),
            "ADR-0011 0001-use-a.md x0001-use-b.md XADR-0001 ADR-00012 ADR-0001x"
        );
        assert_eq!(
            renumber_refs(
                Kind::Issue,
                "blocked-by: [\"#0001\"] x#0001 #00012 ADR-0001",
                1,
                3,
                "fix-a",
                Path::new("docs/issues")
            ),
            "blocked-by: [\"#0003\"] x#0001 #00012 ADR-0001"
        );
    }

    #[test]
    fn status_is_read_from_the_first_status_line_of_the_front_matter() {
        assert_eq!(
            status("---\nstatus: accepted\n---\n").as_deref(),
            Some("accepted")
        );
        assert_eq!(status("# no front matter\nstatus: x\n"), None);
        assert_eq!(status("---\ndate: 2026-10-11\n---\nstatus: x\n"), None);
        assert_eq!(status("---\nstatus:\n---\n"), None);
        assert_eq!(
            status("---\nstatus:\taccepted\t# x\n---\n").as_deref(),
            Some("accepted")
        );
    }

    #[test]
    fn titles_are_one_non_blank_line() {
        assert!(is_title("foo を使う"));
        assert!(!is_title(""));
        assert!(!is_title("  "));
        assert!(!is_title("a\nb"));
    }

    #[test]
    fn template_has_front_matter_title_and_sections() {
        assert_eq!(
            template(7, "foo を使う", "2026-10-11"),
            "---\n\
             status: proposed\n\
             date: 2026-10-11\n\
             requires: []\n\
             supersedes: []\n\
             superseded-by:\n\
             issues: []\n\
             ---\n\
             \n\
             # ADR-0007: foo を使う\n\
             \n\
             ## 背景\n\
             \n\
             ## 検討した案\n\
             \n\
             ## 決定と理由\n\
             \n\
             ### 帰結\n\
             \n\
             ### 確認\n"
        );
    }

    #[test]
    fn record_paths_pad_to_four_digits() {
        assert_eq!(Kind::Adr.path(8, "use-i"), "docs/adr/0008-use-i.md");
        assert_eq!(Kind::Issue.path(8, "fix-i"), "docs/issues/0008-fix-i.md");
    }
}
