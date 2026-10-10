//! 設計の記録（ADR）の形。前付けのキーと status、必須と推奨の節、ファイル名と番号、雛形を持つ（ADR-0009）。
//!
//! 形の値を持つのはこのクレートだけにする。スキル `adr` と `.claude/scripts/branch-topics.sh` は CLI を呼び、
//! claude-hooks はライブラリとしてパスの判定を使う。

pub mod check;
pub mod parse;

/// ADR を置くディレクトリ（リポジトリの直下から）。
pub const ADR_DIR: &str = "docs/adr";

/// 前付けのキー（書く順）。
pub const KEYS: [&str; 6] = [
    "status",
    "date",
    "requires",
    "supersedes",
    "superseded-by",
    "issues",
];

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

/// 確定した後も直してよい節（日付つきの追記）。
pub const NOTES_SECTION: Section = Section {
    ja: "補足",
    en: "More Information",
};

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
pub fn adr_number(name: &str) -> Option<u32> {
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

/// 次の ADR の番号。`paths` のうち `docs/adr/<数字>-*.md` の番号の最大 + 1（4 桁まで。3 桁で振っていた ADR も
/// 数える。5 桁以上は日付などの番号でない数字なので数えない）。無ければ 1。
pub fn next_number<'a>(paths: impl IntoIterator<Item = &'a str>) -> u32 {
    paths
        .into_iter()
        .filter_map(|path| {
            let name = path.strip_prefix(ADR_DIR)?.strip_prefix('/')?;
            let (number, _) = name.strip_suffix(".md")?.split_once('-')?;
            if name.contains('/') || !(1..=4).contains(&number.len()) {
                return None;
            }
            number.parse::<u32>().ok()
        })
        .max()
        .map_or(1, |n| n + 1)
}

/// ADR のファイルのパス（`docs/adr/NNNN-<slug>.md`）。
pub fn adr_path(number: u32, slug: &str) -> String {
    format!("{ADR_DIR}/{number:04}-{slug}.md")
}

/// 新しい ADR の雛形。status は proposed、前付けのキーは空、必須と推奨の見出しを持つ。
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

/// 前付けの最初の `status:` の行の値（後ろの ` # コメント` と引用符は外す）。前付けか値が無ければ `None`。
/// 前付けのほかの行の形は問わない（形を検査していない文書も読むため。形の検査は [`check`] が行う）。
pub fn status(text: &str) -> Option<String> {
    let mut lines = text.lines();
    if lines.next() != Some("---") {
        return None;
    }
    let line = lines
        .take_while(|l| *l != "---")
        .find_map(|l| l.strip_prefix("status:"))?;
    let value = line
        .split(" #")
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches('"');
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
        assert_eq!(adr_number("0013-docs-consistency-check.md"), Some(13));
        assert_eq!(adr_number("0005-k-offset-2d.md"), Some(5));
        assert_eq!(adr_number("README.md"), None);
        assert_eq!(adr_number("013-short.md"), None);
        assert_eq!(adr_number("00013-long.md"), None);
        assert_eq!(adr_number("0013-name.txt"), None);
        assert_eq!(adr_number("0013.md"), None);
        assert_eq!(adr_number("0013-.md"), None);
        assert_eq!(adr_number("0013-Docs-check.md"), None);
        assert_eq!(adr_number("0013-docs_check.md"), None);
        assert_eq!(adr_number("0013-docs--check.md"), None);
        assert_eq!(adr_number("0013-文書.md"), None);
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
        assert_eq!(next_number([]), 1);
        assert_eq!(
            next_number(["docs/adr/0001-a.md", "docs/adr/0003-c.md", "README.md"]),
            4
        );
        // 3 桁で振っていた ADR も数える
        assert_eq!(next_number(["docs/adr/001-a.md", "docs/adr/007-g.md"]), 8);
        // docs/adr の直下でないもの、番号で始まらないものは数えない
        assert_eq!(
            next_number([
                "docs/adr/README.md",
                "docs/adr/drafts/0009-x.md",
                "docs/issues/0042-y.md",
                "docs/adr/0002-b.md",
            ]),
            3
        );
    }

    #[test]
    fn next_number_ignores_numbers_longer_than_four_digits() {
        assert_eq!(
            next_number(["docs/adr/0002-b.md", "docs/adr/20261010-notes.md"]),
            3
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
    fn adr_path_pads_to_four_digits() {
        assert_eq!(adr_path(8, "use-i"), "docs/adr/0008-use-i.md");
    }
}
