//! ADR の検査。ADR のパスと中身の組を受け取り、違反（error）と注意（warning）をすべて返す。

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

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
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        todo!()
    }
}

/// `adrs`（docs/adr の直下の .md のパスと中身）の形と、ADR どうしの参照を確かめる。`issues` は docs/issues の
/// issue の番号で、`None`（docs/issues の無いリポジトリ）なら `issues` の要素は形だけを見る。`scanned` は置き換え
/// られた ADR への参照を探すファイル（docs/adr の中のものは見ない）。
pub fn check(
    _adrs: &BTreeMap<String, String>,
    _issues: Option<&BTreeSet<u32>>,
    _scanned: &BTreeMap<String, String>,
) -> Vec<Finding> {
    todo!()
}

/// 前付けと補足の節を除いた本文が変わったか（確定した ADR に許す変更を除いて比べる）。
pub fn body_changed(_old: &str, _new: &str) -> bool {
    todo!()
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
    fn english_notes_section_is_also_open() {
        let old = BODY.replace("## 補足", "## More Information");
        assert!(!body_changed(
            &old,
            &(old.clone() + "- 2026-10-11: added\n")
        ));
    }

    #[test]
    fn other_changes_change_the_body() {
        assert!(body_changed(BODY, &BODY.replace("文", "別の文")));
        // 補足の後に別の節があれば、その節は本文
        let old = BODY.to_string() + "\n## 付録\n\nx\n";
        assert!(body_changed(&old, &old.replace("\nx\n", "\ny\n")));
    }
}
