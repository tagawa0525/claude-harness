//! docs/ を OKF（Open Knowledge Format）v0.2 のバンドルとして検査する（ADR-0092）。
//!
//! `docs/index.md` のフロントマターに `okf_version` を持つリポジトリだけが対象で、ほかのリポジトリの `docs/` は見ない。

use std::collections::BTreeMap;

use crate::check::Finding;

/// バンドルのルート。
pub const BUNDLE_ROOT: &str = "docs";

/// `files`（リポジトリの直下からのパスと、`docs/` 以下の追跡している .md の中身）の OKF の形の違反と注意。
/// バンドルでなければ（`docs/index.md` が `okf_version` を持たなければ）空。
pub fn check_bundle(files: &BTreeMap<String, String>) -> Vec<Finding> {
    let _ = files;
    Vec::new()
}

/// `files` の索引（`index.md`）に載っていない文書と下位のディレクトリの行を足した、索引のパスと新しい中身。
/// バンドルでなければ空。`index.md` が無いディレクトリは新しく作る。既にある行は直さない。
pub fn complete_indexes(files: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let _ = files;
    BTreeMap::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::Severity;

    fn bundle(files: &[(&str, &str)]) -> BTreeMap<String, String> {
        files
            .iter()
            .map(|(p, t)| (p.to_string(), t.to_string()))
            .collect()
    }

    const ROOT_INDEX: &str =
        "---\nokf_version: \"0.2\"\n---\n\n# docs\n\n* [adr](adr/) - 決定の記録\n";
    const ADR_INDEX: &str = "# adr\n\n* [ADR-0001: foo](0001-foo.md)\n";
    const ADR: &str = "---\ntype: ADR\nstatus: accepted\n---\n\n# ADR-0001: foo\n";

    fn valid() -> Vec<(&'static str, &'static str)> {
        vec![
            ("docs/index.md", ROOT_INDEX),
            ("docs/adr/index.md", ADR_INDEX),
            ("docs/adr/0001-foo.md", ADR),
        ]
    }

    fn errors(files: &[(&str, &str)]) -> Vec<String> {
        check_bundle(&bundle(files))
            .into_iter()
            .filter(|f| f.severity == Severity::Error)
            .map(|f| f.to_string())
            .collect()
    }

    fn warnings(files: &[(&str, &str)]) -> Vec<String> {
        check_bundle(&bundle(files))
            .into_iter()
            .filter(|f| f.severity == Severity::Warning)
            .map(|f| f.to_string())
            .collect()
    }

    fn with(replacing: &'static str, text: &'static str) -> Vec<(&'static str, &'static str)> {
        let mut files = valid();
        files.retain(|(p, _)| *p != replacing);
        files.push((replacing, text));
        files
    }

    #[test]
    #[ignore = "RED: ADR-0092 の実装まで"]
    fn a_valid_bundle_has_no_findings() {
        assert_eq!(errors(&valid()), Vec::<String>::new());
        assert_eq!(warnings(&valid()), Vec::<String>::new());
    }

    #[test]
    #[ignore = "RED: ADR-0092 の実装まで"]
    fn a_docs_dir_without_okf_version_is_not_a_bundle() {
        let files = [
            ("docs/index.md", "# docs\n"),
            ("docs/note.md", "no front matter\n"),
        ];
        assert_eq!(errors(&files), Vec::<String>::new());
        assert_eq!(errors(&[("docs/note.md", "x\n")]), Vec::<String>::new());
    }

    #[test]
    #[ignore = "RED: ADR-0092 の実装まで"]
    fn a_concept_needs_front_matter_with_a_type() {
        let no_front = with("docs/adr/0001-foo.md", "# ADR-0001: foo\n");
        assert_eq!(errors(&no_front).len(), 1, "{:?}", errors(&no_front));
        assert!(errors(&no_front)[0].contains("docs/adr/0001-foo.md"));
        let no_type = with(
            "docs/adr/0001-foo.md",
            "---\nstatus: accepted\n---\n\n# ADR-0001: foo\n",
        );
        assert!(errors(&no_type)[0].contains("`type`"));
        let empty_type = with(
            "docs/adr/0001-foo.md",
            "---\ntype:\n---\n\n# ADR-0001: foo\n",
        );
        assert!(errors(&empty_type)[0].contains("`type`"));
    }

    #[test]
    #[ignore = "RED: ADR-0092 の実装まで"]
    fn front_matter_with_nested_yaml_is_read_by_its_top_level_keys() {
        let nested = "---\ntype: Metric\nsources:\n  - id: a\n    resource: https://x\ngenerated: { by: human:a, at: 2026-11-16T01:03:00+09:00 }\n---\n\n# m\n";
        let mut files = valid();
        files.push(("docs/adr/metric.md", nested));
        files.retain(|(p, _)| *p != "docs/adr/index.md");
        files.push((
            "docs/adr/index.md",
            "# adr\n\n* [ADR-0001: foo](0001-foo.md)\n* [m](metric.md)\n",
        ));
        assert_eq!(errors(&files), Vec::<String>::new());
    }

    #[test]
    #[ignore = "RED: ADR-0092 の実装まで"]
    fn an_adr_must_have_type_adr_and_an_issue_type_issue() {
        let wrong = with(
            "docs/adr/0001-foo.md",
            "---\ntype: Note\nstatus: accepted\n---\n\n# ADR-0001: foo\n",
        );
        assert!(
            errors(&wrong)[0].contains("`type: ADR`"),
            "{:?}",
            errors(&wrong)
        );

        let mut files = valid();
        files.push(("docs/issues/0001-bar.md", "---\ntype: ADR\n---\n\n# bar\n"));
        files.push(("docs/issues/index.md", "# issues\n\n* [bar](0001-bar.md)\n"));
        files.retain(|(p, _)| *p != "docs/index.md");
        files.push((
            "docs/index.md",
            "---\nokf_version: \"0.2\"\n---\n\n* [adr](adr/)\n* [issues](issues/)\n",
        ));
        let found = errors(&files);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("`type: Issue`"));
    }

    #[test]
    #[ignore = "RED: ADR-0092 の実装まで"]
    fn a_directory_with_concepts_needs_an_index() {
        let mut files = valid();
        files.retain(|(p, _)| *p != "docs/adr/index.md");
        let found = errors(&files);
        assert!(
            found
                .iter()
                .any(|m| m.contains("docs/adr/index.md") && m.contains("no index")),
            "{found:?}"
        );
    }

    #[test]
    #[ignore = "RED: ADR-0092 の実装まで"]
    fn an_index_has_no_front_matter_except_the_root_okf_version() {
        let found = errors(&with(
            "docs/adr/index.md",
            "---\ntype: Index\n---\n\n* [ADR-0001: foo](0001-foo.md)\n",
        ));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("docs/adr/index.md"));
        let root = "---\nokf_version: \"0.2\"\ntitle: x\n---\n\n* [adr](adr/)\n";
        let found = errors(&with("docs/index.md", root));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("docs/index.md"));
    }

    #[test]
    #[ignore = "RED: ADR-0092 の実装まで"]
    fn an_index_must_list_every_concept_and_subdirectory() {
        let found = errors(&with("docs/adr/index.md", "# adr\n"));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("0001-foo.md") && found[0].contains("not listed"));
        let found = errors(&with(
            "docs/index.md",
            "---\nokf_version: \"0.2\"\n---\n\n# docs\n",
        ));
        assert!(
            found[0].contains("adr/") && found[0].contains("not listed"),
            "{found:?}"
        );
    }

    #[test]
    #[ignore = "RED: ADR-0092 の実装まで"]
    fn an_index_link_must_point_to_an_existing_file() {
        let found = errors(&with(
            "docs/adr/index.md",
            "# adr\n\n* [ADR-0001: foo](0001-foo.md)\n* [gone](0002-gone.md)\n",
        ));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("0002-gone.md") && found[0].contains("does not exist"));
    }

    #[test]
    #[ignore = "RED: ADR-0092 の実装まで"]
    fn an_index_entry_title_and_description_follow_the_target() {
        let titled = "---\ntype: Guide\ntitle: Windows\ndescription: 入れ方\n---\n\n# w\n";
        let mut files = valid();
        files.push(("docs/windows.md", titled));
        files.retain(|(p, _)| *p != "docs/index.md");
        files.push((
            "docs/index.md",
            "---\nokf_version: \"0.2\"\n---\n\n* [adr](adr/)\n* [Windows](windows.md) - 入れ方\n",
        ));
        assert_eq!(errors(&files), Vec::<String>::new());
        assert_eq!(warnings(&files), Vec::<String>::new());

        files.retain(|(p, _)| *p != "docs/index.md");
        files.push((
            "docs/index.md",
            "---\nokf_version: \"0.2\"\n---\n\n* [adr](adr/)\n* [Win](windows.md) - 別\n",
        ));
        let found = warnings(&files);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found.iter().any(|m| m.contains("title")));
        assert!(found.iter().any(|m| m.contains("description")));

        // 題は、フロントマターの title が無ければ最初の 1 段目の見出しと比べる
        let found = warnings(&with(
            "docs/adr/index.md",
            "# adr\n\n* [ADR-0001: other](0001-foo.md)\n",
        ));
        assert_eq!(found.len(), 1, "{found:?}");
    }

    #[test]
    #[ignore = "RED: ADR-0092 の実装まで"]
    fn log_headings_are_iso_dates() {
        let mut files = valid();
        files.push((
            "docs/adr/log.md",
            "# Log\n\n## 2026-11-17\n* **Update**: x\n\n## yesterday\n",
        ));
        let found = errors(&files);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("docs/adr/log.md") && found[0].contains("YYYY-MM-DD"));
    }

    #[test]
    #[ignore = "RED: ADR-0092 の実装まで"]
    fn complete_indexes_adds_missing_entries_and_creates_missing_indexes() {
        let files = bundle(&[
            (
                "docs/index.md",
                "---\nokf_version: \"0.2\"\n---\n\n# docs\n",
            ),
            ("docs/adr/0001-foo.md", ADR),
            (
                "docs/windows.md",
                "---\ntype: Guide\ntitle: Windows\ndescription: 入れ方\n---\n",
            ),
        ]);
        let done = complete_indexes(&files);
        assert_eq!(
            done["docs/index.md"],
            "---\nokf_version: \"0.2\"\n---\n\n# docs\n\n\
             * [adr](adr/)\n\
             * [Windows](windows.md) - 入れ方\n"
        );
        assert_eq!(
            done["docs/adr/index.md"],
            "# adr\n\n* [ADR-0001: foo](0001-foo.md)\n"
        );
        let mut all = files.clone();
        all.extend(done);
        assert_eq!(
            check_bundle(&all)
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>(),
            Vec::<String>::new()
        );
    }

    #[test]
    #[ignore = "RED: ADR-0092 の実装まで"]
    fn complete_indexes_keeps_existing_lines_and_ignores_non_bundles() {
        let files = bundle(&valid().into_iter().collect::<Vec<_>>());
        assert!(complete_indexes(&files).is_empty());
        let mut more = files.clone();
        more.insert(
            "docs/adr/0002-bar.md".to_string(),
            ADR.replace("0001: foo", "0002: bar"),
        );
        let done = complete_indexes(&more);
        assert_eq!(
            done["docs/adr/index.md"],
            "# adr\n\n* [ADR-0001: foo](0001-foo.md)\n* [ADR-0002: bar](0002-bar.md)\n"
        );
        assert!(complete_indexes(&bundle(&[("docs/a.md", "x")])).is_empty());
    }
}
