//! docs/ を OKF（Open Knowledge Format）v0.2 のバンドルとして検査する（ADR-0092）。
//!
//! `docs/index.md` のフロントマターに `okf_version` を持つリポジトリだけが対象で、ほかのリポジトリの `docs/` は見ない。
//! フロントマターは OKF が許す YAML 全体（入れ子、複数行の一覧）を読めるよう、字下げのない `key: value` の行だけを見る。

use std::collections::{BTreeMap, BTreeSet};

use crate::check::{Finding, Severity, is_date};
use crate::{Kind, is_adr_path, record_name};

/// バンドルのルート。
pub const BUNDLE_ROOT: &str = "docs";

const INDEX: &str = "index.md";
const LOG: &str = "log.md";

/// フロントマターの字下げのない `key: value`（引用符は外す）。フロントマターが無いか閉じていなければ `None`。
fn top_level(text: &str) -> Option<BTreeMap<String, String>> {
    let mut lines = text.lines();
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let mut keys = BTreeMap::new();
    for line in lines {
        if line.trim_end() == "---" {
            return Some(keys);
        }
        if line.starts_with([' ', '\t', '-', '#']) {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let value = value.trim();
            let value = ['"', '\'']
                .iter()
                .find_map(|q| value.strip_prefix(*q).and_then(|v| v.strip_suffix(*q)))
                .unwrap_or(value);
            keys.insert(key.trim().to_string(), value.to_string());
        }
    }
    None
}

/// `docs/index.md` の中身が `okf_version` を持つか（バンドルのルートか）。
pub fn is_bundle_index(text: &str) -> bool {
    top_level(text).is_some_and(|front| front.contains_key("okf_version"))
}

fn is_bundle(files: &BTreeMap<String, String>) -> bool {
    files
        .get(&format!("{BUNDLE_ROOT}/{INDEX}"))
        .is_some_and(|text| is_bundle_index(text))
}

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

fn name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// 文書か下位のディレクトリを持つディレクトリ（バンドルのルートを含む）。
fn directories(files: &BTreeMap<String, String>) -> BTreeSet<String> {
    let mut dirs = BTreeSet::new();
    dirs.insert(BUNDLE_ROOT.to_string());
    for path in files.keys() {
        let mut dir = dir_of(path);
        while dir.len() > BUNDLE_ROOT.len() {
            dirs.insert(dir.to_string());
            dir = dir_of(dir);
        }
    }
    dirs
}

/// `dir` の索引に載せるもの。下位のディレクトリ（`name/`）と、`index.md`、`log.md` 以外の文書（`name.md`）。
fn expected_entries(
    files: &BTreeMap<String, String>,
    dirs: &BTreeSet<String>,
    dir: &str,
) -> Vec<String> {
    let mut out: Vec<String> = dirs
        .iter()
        .filter(|d| dir_of(d) == dir && d.as_str() != dir)
        .map(|d| format!("{}/", name_of(d)))
        .collect();
    out.extend(
        files
            .keys()
            .filter(|p| dir_of(p) == dir && ![INDEX, LOG].contains(&name_of(p)))
            .map(|p| name_of(p).to_string()),
    );
    out
}

/// 索引の 1 行（`* [題](リンク) - 説明`）。
struct Entry<'a> {
    line: usize,
    title: &'a str,
    url: &'a str,
    description: Option<&'a str>,
}

fn entries(text: &str) -> Vec<Entry<'_>> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let Some(rest) = line
            .strip_prefix("* [")
            .or_else(|| line.strip_prefix("- ["))
        else {
            continue;
        };
        let Some((title, rest)) = rest.split_once("](") else {
            continue;
        };
        let Some((url, rest)) = rest.split_once(')') else {
            continue;
        };
        out.push(Entry {
            line: i + 1,
            title,
            url,
            description: rest.strip_prefix(" - ").map(str::trim),
        });
    }
    out
}

/// 索引（`index_dir` のもの）のリンクが指す、バンドルの中のパス。ディレクトリなら末尾に `/` を付ける。外部のリンクと
/// 文書でもディレクトリでもないリンクは `None`。
fn resolve(index_dir: &str, url: &str) -> Option<String> {
    let url = url.split('#').next().unwrap_or("");
    if url.contains("://") || !(url.ends_with(".md") || url.ends_with('/')) {
        return None;
    }
    let joined = match url.strip_prefix('/') {
        Some(rest) => format!("{BUNDLE_ROOT}/{rest}"),
        None => format!("{index_dir}/{url}"),
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    let mut path = parts.join("/");
    if url.ends_with('/') {
        path.push('/');
    }
    Some(path)
}

/// 索引に載っている行のリンク先（`index_dir` からの名前。`name.md` か `name/`）。
fn listed(index_dir: &str, text: &str) -> BTreeSet<String> {
    entries(text)
        .iter()
        .filter_map(|e| resolve(index_dir, e.url))
        .filter_map(|p| {
            let rest = p.strip_prefix(index_dir)?.strip_prefix('/')?;
            (!rest.is_empty()).then(|| rest.to_string())
        })
        .collect()
}

/// 文書の題（フロントマターの `title`、無ければ最初の 1 段目の見出し、無ければファイル名）。
fn title_of(path: &str, text: &str) -> String {
    if let Some(title) = top_level(text)
        .and_then(|f| f.get("title").cloned())
        .filter(|t| !t.is_empty())
    {
        return title;
    }
    // フロントマターの YAML のコメント（`# ...`）を題と読まないよう、フロントマターの後ろの行だけを見る
    let body = text
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n").map(|(_, body)| body))
        .unwrap_or(text);
    body.lines()
        .find_map(|l| l.strip_prefix("# "))
        .map(|t| t.trim().to_string())
        .unwrap_or_else(|| name_of(path).trim_end_matches(".md").to_string())
}

fn description_of(text: &str) -> Option<String> {
    top_level(text)?
        .remove("description")
        .filter(|d| !d.is_empty())
}

fn finding(severity: Severity, path: &str, line: Option<usize>, message: impl ToString) -> Finding {
    Finding {
        path: path.to_string(),
        line,
        severity,
        message: message.to_string(),
    }
}

/// `files`（リポジトリの直下からのパスと、`docs/` 以下の追跡している .md の中身）の OKF の形の違反と注意。
/// バンドルでなければ（`docs/index.md` が `okf_version` を持たなければ）空。
pub fn check_bundle(files: &BTreeMap<String, String>) -> Vec<Finding> {
    if !is_bundle(files) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (path, text) in files {
        match name_of(path) {
            INDEX => check_index_front(path, text, &mut out),
            LOG => check_log(path, text, &mut out),
            _ => check_concept(path, text, &mut out),
        }
    }
    let dirs = directories(files);
    for dir in &dirs {
        let index_path = format!("{dir}/{INDEX}");
        let Some(index) = files.get(&index_path) else {
            out.push(finding(
                Severity::Error,
                &index_path,
                None,
                format!("{dir}/ has documents but no index.md"),
            ));
            continue;
        };
        check_index(files, &dirs, dir, &index_path, index, &mut out);
    }
    out.sort();
    out
}

/// `index.md` のフロントマターは、バンドルのルートの `okf_version` だけ。
fn check_index_front(path: &str, text: &str, out: &mut Vec<Finding>) {
    let Some(front) = top_level(text) else {
        return;
    };
    let root = path == format!("{BUNDLE_ROOT}/{INDEX}");
    if !root || front.keys().any(|k| k != "okf_version") {
        out.push(finding(
            Severity::Error,
            path,
            None,
            "an index.md has no front matter except `okf_version` in the bundle root",
        ));
    }
}

/// `log.md` の日付の見出し。
fn check_log(path: &str, text: &str, out: &mut Vec<Finding>) {
    for (i, line) in text.lines().enumerate() {
        if let Some(date) = line.strip_prefix("## ")
            && !is_date(date.trim())
        {
            out.push(finding(
                Severity::Error,
                path,
                Some(i + 1),
                format!("a log heading must be an ISO 8601 date (YYYY-MM-DD): {date:?}"),
            ));
        }
    }
}

/// `dir` の索引が、同じディレクトリの文書と下位のディレクトリをすべて載せ、リンクが実在し、題と説明がリンク先と合うか。
fn check_index(
    files: &BTreeMap<String, String>,
    dirs: &BTreeSet<String>,
    dir: &str,
    index_path: &str,
    index: &str,
    out: &mut Vec<Finding>,
) {
    let listed = listed(dir, index);
    for name in expected_entries(files, dirs, dir) {
        if !listed.contains(&name) {
            out.push(finding(
                Severity::Error,
                index_path,
                None,
                format!("`{name}` is not listed in the index"),
            ));
        }
    }
    for entry in entries(index) {
        let Some(target) = resolve(dir, entry.url) else {
            continue;
        };
        let exists = match target.strip_suffix('/') {
            Some(d) => dirs.contains(d),
            None => files.contains_key(&target),
        };
        if !exists {
            out.push(finding(
                Severity::Error,
                index_path,
                Some(entry.line),
                format!("the link target `{}` does not exist", entry.url),
            ));
            continue;
        }
        let Some(text) = files.get(&target) else {
            continue;
        };
        let title = title_of(&target, text);
        let description = description_of(text);
        let mut mismatches = Vec::new();
        if entry.title != title {
            mismatches.push(format!(
                "title {:?} but the document's is {title:?}",
                entry.title
            ));
        }
        if entry.description != description.as_deref() {
            mismatches.push(format!(
                "description {:?} but the document's is {description:?}",
                entry.description
            ));
        }
        // check --changed は FILE のパスの warning だけを残すので、索引と文書のどちらを直しても見えるよう両方に出す
        for mismatch in mismatches {
            out.push(finding(
                Severity::Warning,
                index_path,
                Some(entry.line),
                format!("the entry of `{}` has the {mismatch}", entry.url),
            ));
            out.push(finding(
                Severity::Warning,
                &target,
                None,
                format!("the entry in `{index_path}` has the {mismatch}"),
            ));
        }
    }
}

/// 概念の文書のフロントマターと `type`。ADR は `type: ADR`、`docs/issues/` の issue は `type: Issue`。
fn check_concept(path: &str, text: &str, out: &mut Vec<Finding>) {
    let required = if is_adr_path(path) {
        Some("ADR")
    } else if record_name(Kind::Issue, path).is_some() {
        Some("Issue")
    } else {
        None
    };
    let error = |message: String| finding(Severity::Error, path, None, message);
    let Some(front) = top_level(text) else {
        out.push(error(
            "a concept needs a YAML front matter with `type`".to_string(),
        ));
        return;
    };
    match (front.get("type").map(String::as_str), required) {
        (None | Some(""), Some(want)) => out.push(error(format!(
            "the front matter has no `type` (this document needs `type: {want}`)"
        ))),
        (None | Some(""), None) => out.push(error("the front matter has no `type`".to_string())),
        (Some(got), Some(want)) if got != want => out.push(error(format!(
            "this document needs `type: {want}`, not `{got}`"
        ))),
        _ => {}
    }
}

/// `files` の索引（`index.md`）に載っていない文書と下位のディレクトリの行を足した、索引のパスと新しい中身。
/// バンドルでなければ空。`index.md` が無いディレクトリは新しく作る。既にある行は直さない。変わらない索引は含めない。
pub fn complete_indexes(files: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if !is_bundle(files) {
        return out;
    }
    let dirs = directories(files);
    for dir in &dirs {
        let index_path = format!("{dir}/{INDEX}");
        let existing = files.get(&index_path);
        let listed = existing.map(|t| listed(dir, t)).unwrap_or_default();
        let lines: Vec<String> = expected_entries(files, &dirs, dir)
            .into_iter()
            .filter(|name| !listed.contains(name))
            .map(|name| entry_line(files, dir, &name))
            .collect();
        if lines.is_empty() && existing.is_some() {
            continue;
        }
        let mut text = existing
            .cloned()
            .unwrap_or_else(|| format!("# {}\n", name_of(dir)));
        if !text.ends_with('\n') {
            text.push('\n');
        }
        // 行の一覧が続いているときは空行を挟まず、見出しなどの後なら 1 行空ける
        let last = text.lines().rev().find(|l| !l.trim().is_empty());
        if !lines.is_empty() && !last.is_some_and(|l| l.starts_with("* [")) {
            text.push('\n');
        }
        for line in lines {
            text.push_str(&line);
            text.push('\n');
        }
        out.insert(index_path, text);
    }
    out
}

/// `dir` の索引に足す、`name`（`sub/` か `file.md`）の行。
fn entry_line(files: &BTreeMap<String, String>, dir: &str, name: &str) -> String {
    if let Some(sub) = name.strip_suffix('/') {
        return format!("* [{sub}]({name})");
    }
    let text = &files[&format!("{dir}/{name}")];
    let title = title_of(name, text);
    match description_of(text) {
        Some(description) => format!("* [{title}]({name}) - {description}"),
        None => format!("* [{title}]({name})"),
    }
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
    fn a_valid_bundle_has_no_findings() {
        assert_eq!(errors(&valid()), Vec::<String>::new());
        assert_eq!(warnings(&valid()), Vec::<String>::new());
    }

    #[test]
    fn a_docs_dir_without_okf_version_is_not_a_bundle() {
        let files = [
            ("docs/index.md", "# docs\n"),
            ("docs/note.md", "no front matter\n"),
        ];
        assert_eq!(errors(&files), Vec::<String>::new());
        assert_eq!(errors(&[("docs/note.md", "x\n")]), Vec::<String>::new());
    }

    #[test]
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
    fn an_index_link_must_point_to_an_existing_file() {
        let found = errors(&with(
            "docs/adr/index.md",
            "# adr\n\n* [ADR-0001: foo](0001-foo.md)\n* [gone](0002-gone.md)\n",
        ));
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("0002-gone.md") && found[0].contains("does not exist"));
    }

    #[test]
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
        assert_eq!(found.len(), 4, "{found:?}");
        assert!(found.iter().any(|m| m.contains("title")));
        assert!(found.iter().any(|m| m.contains("description")));

        // 題は、フロントマターの title が無ければ最初の 1 段目の見出しと比べる
        let found = warnings(&with(
            "docs/adr/index.md",
            "# adr\n\n* [ADR-0001: other](0001-foo.md)\n",
        ));
        assert_eq!(found.len(), 2, "{found:?}");
    }

    #[test]
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

    #[test]
    fn titles_skip_front_matter_comments_and_dash_entries_count() {
        let doc = "---
type: Guide
# a comment
---

# Real
";
        let files = bundle(&[
            (
                "docs/index.md",
                "---
okf_version: \"0.2\"
---

- [Real](a.md)
",
            ),
            ("docs/a.md", doc),
        ]);
        assert_eq!(check_bundle(&files), vec![]);
    }

    #[test]
    fn an_entry_description_needs_a_description_in_the_document() {
        let found = warnings(&with(
            "docs/adr/index.md",
            "# adr

* [ADR-0001: foo](0001-foo.md) - stale
",
        ));
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(
            found
                .iter()
                .any(|m| m.starts_with("docs/adr/0001-foo.md: warning"))
        );
    }
}
