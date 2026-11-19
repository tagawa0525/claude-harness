//! docs/ を OKF（Open Knowledge Format）v0.3 のバンドルとして検査する（ADR-0092）。`okf_version` が "0.2" のバンドルには、
//! v0.2 と違うルートの `index.md` の規則だけを v0.2 のものにする。
//!
//! 節の番号は v0.3 の仕様のもの。仕様は https://github.com/open-knowledge-format/open-knowledge-format の `SPEC.md`
//! （コミット 527e81af、2026-11-19 取得）。v0.2 は https://github.com/GoogleCloudPlatform/open-knowledge-format の
//! `SPEC.md`（コミット 8a8ff87b、2026-11-19 取得）。
//!
//! `docs/index.md` のフロントマターに `okf_version` を持つリポジトリだけが対象で、ほかのリポジトリの `docs/` は見ない。
//! フロントマターは OKF が許す YAML 全体（入れ子、複数行の一覧）を読めるよう、字下げのない `key: value` の行だけを見る。

use std::collections::{BTreeMap, BTreeSet};

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use crate::check::{Finding, Severity, is_date};
use crate::{Kind, is_adr_path, record_name};

/// バンドルのルート。
pub const BUNDLE_ROOT: &str = "docs";

const INDEX: &str = "index.md";
const LOG: &str = "log.md";

/// 検査の規則を知っている `okf_version` の値。
const KNOWN_VERSIONS: [&str; 2] = ["0.2", "0.3"];

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

/// リポジトリの直下 `root` の `docs/` が OKF のバンドルか。`docs/index.md` が無ければバンドルでない。あるのに読めなければ
/// 誤りにする（バンドルでないものとして進めると、後の索引の読み取りと判定が食い違う）。
pub fn is_bundle_at(root: &std::path::Path) -> Result<bool, String> {
    let path = format!("{BUNDLE_ROOT}/{INDEX}");
    match std::fs::read_to_string(root.join(&path)) {
        Ok(text) => Ok(is_bundle_index(&text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("{path}: {e}")),
    }
}

/// バンドルの `okf_version` の値（後ろの YAML のコメントと引用符を除く）。バンドルでなければ `None`。
fn version(files: &BTreeMap<String, String>) -> Option<String> {
    let raw = top_level(files.get(&format!("{BUNDLE_ROOT}/{INDEX}"))?)?.remove("okf_version")?;
    // コメントを除くのは版の値だけ。top_level は title などの引用符の無い `#12` を消さないよう、# を値の一部として読む
    let value = raw.split('#').next().unwrap_or("").trim();
    Some(
        ['"', '\'']
            .iter()
            .find_map(|q| value.strip_prefix(*q).and_then(|v| v.strip_suffix(*q)))
            .unwrap_or(value)
            .to_string(),
    )
}

/// リポジトリの直下からのパスが、バンドルの一部か（名前が `.` で始まるファイルとディレクトリの下でない、OKF v0.3 §3.2）。
pub fn in_bundle(path: &str) -> bool {
    !path.split('/').any(|part| part.starts_with('.'))
}

fn is_bundle(files: &BTreeMap<String, String>) -> bool {
    version(files).is_some()
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
    /// 題の表示の文字列（`plain_text`）
    title: String,
    url: String,
    description: Option<&'a str>,
    /// 行のうち、リンクの終わりまで（行頭の記号を含む）と、その後ろ
    head: &'a str,
    tail: &'a str,
    /// Markdown がこの行を `url` へのリンクと読むか
    markdown: bool,
}

fn entries(text: &str) -> Vec<Entry<'_>> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let Some(item) = line
            .strip_prefix("* ")
            .or_else(|| line.strip_prefix("- "))
            .filter(|item| item.starts_with('['))
        else {
            continue;
        };
        let (title, url, after, markdown) = match leading_link(item) {
            Some((title, url, end)) => (title, url, &item[end..], true),
            // Markdown のリンクとして読めない手書きの行（対にならない `[`、説明まで続くバッククォート）も、題の中の
            // 最初の `](` で切って載っているとし、注意を出す。載っていないとすると、index が同じ文書の行を足し直す
            None => {
                let Some((title, rest)) = item[1..].split_once("](") else {
                    continue;
                };
                let Some((url, after)) = rest.split_once(')') else {
                    continue;
                };
                (plain_text(title), url.to_string(), after, false)
            }
        };
        let (head, tail) = line.split_at(line.len() - after.len());
        out.push(Entry {
            line: i + 1,
            title,
            url,
            description: tail.strip_prefix(" - ").map(str::trim),
            head,
            tail,
            markdown,
        });
    }
    out
}

/// `md` の先頭から始まるインラインのリンクの、表示の文字列（`plain_text`）、リンク先、`md` の中のリンクの終わり。
fn leading_link(md: &str) -> Option<(String, String, usize)> {
    let mut events = Parser::new(md)
        .into_offset_iter()
        .filter(|(e, _)| !matches!(e, Event::Start(Tag::Paragraph)));
    let (Event::Start(Tag::Link { dest_url, .. }), range) = events.next()? else {
        return None;
    };
    if range.start != 0 {
        return None;
    }
    let mut title = String::new();
    for (event, _) in events {
        if event == Event::End(TagEnd::Link) {
            return Some((title, dest_url.into_string(), range.end));
        }
        push_plain(&mut title, &event);
    }
    None
}

/// 題（見出しの文字列か、フロントマターの `title`）を見出しのインラインとして読んだ事象。見出しとして読むのは、題だけを
/// 読むと `1. 手順` のような題を一覧の行と読むため。末尾の ` #` は閉じの列として外す（題の末尾の ` #` を残す）。
fn title_events(md: &str) -> Vec<Event<'static>> {
    Parser::new(&format!("# {md} #"))
        .filter(|e| {
            !matches!(
                e,
                Event::Start(Tag::Heading { .. }) | Event::End(TagEnd::Heading(_))
            )
        })
        .map(Event::into_static)
        .collect()
}

fn push_plain(out: &mut String, event: &Event<'_>) {
    match event {
        Event::Text(t) | Event::Code(t) => out.push_str(t),
        Event::SoftBreak | Event::HardBreak => out.push(' '),
        _ => {}
    }
}

/// 題の表示の文字列。リンク、強調、コードスパン、画像の印を外し、文字（画像は代わりの文字列）だけを残す。
fn plain_text(md: &str) -> String {
    let mut out = String::new();
    for event in title_events(md) {
        push_plain(&mut out, &event);
    }
    out
}

/// 索引の行に書く題。題を CommonMark で読み、Markdown に書き直す。リンクの文字列にリンクは入れられない（入れると外側の
/// 行がリンクにならない）ので、題のリンク（自動リンクを含む）はその文字列にする。強調も文字だけにする（区切りを書き直すと、
/// 隣り合う強調の区切りが 1 つの並びになり、別の強調に読まれる）。文字のうち、行のリンクの文字列を閉じるか、ほかの構文
/// （コードスパン、強調、HTML、文字参照、エスケープ）を始めうる記号はエスケープする。コードスパン、画像、HTML は残す。
fn entry_title(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    let mut images = Vec::new();
    for event in title_events(md) {
        match event {
            Event::Text(t) => escape_text(&mut out, &t),
            Event::Code(c) => code_span(&mut out, &c),
            Event::InlineHtml(h) | Event::Html(h) => out.push_str(&h),
            Event::SoftBreak | Event::HardBreak => out.push(' '),
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) => {
                out.push_str("![");
                images.push((dest_url, title));
            }
            Event::End(TagEnd::Image) => {
                // パーサーは画像の終わりを始まりの後にだけ出す
                let Some((dest, title)) = images.pop() else {
                    continue;
                };
                out.push_str("](");
                for c in dest.chars() {
                    match c {
                        '(' | ')' | '<' | '>' | '\\' | '&' => {
                            out.push('\\');
                            out.push(c);
                        }
                        // リンク先に書けない空白は ASCII のものだけ（ほかの空白はそのまま書ける）
                        c if c.is_ascii_whitespace() => out.push_str(&format!("%{:02X}", c as u32)),
                        c => out.push(c),
                    }
                }
                if !title.is_empty() {
                    out.push_str(" \"");
                    for c in title.chars() {
                        if matches!(c, '\\' | '"' | '&') {
                            out.push('\\');
                        }
                        out.push(c);
                    }
                    out.push('"');
                }
                out.push(')');
            }
            _ => {}
        }
    }
    out
}

// `&` は続く文字によらずエスケープする。パーサーは文字を文字参照やリンクの境で別の事象に切るので、続く文字を 1 つの
// 事象の中で見ても、次の事象の文字と文字参照になるかは分からない
fn escape_text(out: &mut String, text: &str) {
    for c in text.chars() {
        if "\\`*_[]<&".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
}

/// コードスパンを書く。中身のバッククォートの並びより長い並びで囲み、中身の端がバッククォートか、両端が空白なら、
/// CommonMark が読むときに 1 つずつ外す空白を足す。
fn code_span(out: &mut String, code: &str) {
    let mut longest = 0;
    let mut run = 0;
    for c in code.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat(longest + 1);
    let pad = code.starts_with('`')
        || code.ends_with('`')
        || (code.starts_with(' ') && code.ends_with(' ') && !code.trim().is_empty());
    let space = if pad { " " } else { "" };
    out.push_str(&format!("{fence}{space}{code}{space}{fence}"));
}

/// 索引（`index_dir` のもの）のリンクが指す、バンドルの中のパス。ディレクトリなら末尾に `/` を付ける。外部のリンク
/// （バンドルのルートの外に解けるものと、名前が `.` で始まるパスの下を含む、OKF v0.3 §3.2、§6）と、文書でもディレクトリでもない
/// リンクは `None`。
fn resolve(index_dir: &str, url: &str) -> Option<String> {
    let url = url.split('#').next().unwrap_or("");
    if url.contains("://") || !(url.ends_with(".md") || url.ends_with('/')) {
        return None;
    }
    let url = percent_decode(url);
    let joined = match url.strip_prefix('/') {
        Some(rest) => format!("{BUNDLE_ROOT}/{rest}"),
        None => format!("{index_dir}/{url}"),
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    if parts.first() != Some(&BUNDLE_ROOT) || !in_bundle(&parts.join("/")) {
        return None;
    }
    let mut path = parts.join("/");
    if url.ends_with('/') {
        path.push('/');
    }
    Some(path)
}

/// リンク先の `%XX` を復号する（OKF v0.3 §6）。復号して UTF-8 にならなければ、そのまま返す。
fn percent_decode(url: &str) -> String {
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let bytes = url.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if let [b'%', hi, lo, ..] = bytes[i..]
            && let (Some(hi), Some(lo)) = (hex(hi), hex(lo))
        {
            out.push(hi << 4 | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| url.to_string())
}

/// 索引の行のリンク先に書く形。ASCII の英数字と `-._~/` のほかの ASCII の文字をパーセントエンコードする（OKF v0.3 §6）。
/// ASCII でない文字は、§6 がエンコードを求める空白と予約文字に当たらず、日本語の名前を読めるよう、そのまま書く。
fn percent_encode(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if !c.is_ascii() || c.is_ascii_alphanumeric() || "-._~/".contains(c) {
            out.push(c);
        } else {
            out.push_str(&format!("%{:02X}", c as u32));
        }
    }
    out
}

/// 索引に載っている行のリンク先（`index_dir` からの名前。`name.md` か `name/`）。
fn listed(index_dir: &str, text: &str) -> BTreeSet<String> {
    entries(text)
        .iter()
        .filter_map(|e| resolve(index_dir, &e.url))
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
    let Some(version) = version(files) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    // OKF §12 は consumer に知らない版のバンドルを拒まないよう求めるが、これは書く側のコミットの検査で、知らない版に
    // 今の規則を黙って当てると合否を誤る。違反にして、ほかの検査は v0.3 の規則で続ける
    if !KNOWN_VERSIONS.contains(&version.as_str()) {
        out.push(finding(
            Severity::Error,
            &format!("{BUNDLE_ROOT}/{INDEX}"),
            None,
            format!(
                "design-records knows the rules of `okf_version` {} but not {version:?}; fix the \
                 version or update design-records",
                KNOWN_VERSIONS.map(|v| format!("{v:?}")).join(" and ")
            ),
        ));
    }
    for (path, text) in files {
        match name_of(path) {
            INDEX => check_index_front(path, text, &version, &mut out),
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

/// `index.md` のフロントマターは、バンドルのルートだけが持つ（OKF v0.3 §8）。ルートのキーは v0.3 では
/// `okf_version`、`title`、`description`、`license` と拡張のキー（§8.1）で、v0.2 では `okf_version` だけ（v0.2 §8）。
fn check_index_front(path: &str, text: &str, version: &str, out: &mut Vec<Finding>) {
    let Some(front) = top_level(text) else {
        return;
    };
    let message = if path != format!("{BUNDLE_ROOT}/{INDEX}") {
        "an index.md outside the bundle root has no front matter"
    } else if version == "0.2" && front.keys().any(|k| k != "okf_version") {
        "the bundle-root index.md of an `okf_version: \"0.2\"` bundle has no front matter except \
         `okf_version` (OKF v0.3 permits `title`, `description` and `license`)"
    } else {
        return;
    };
    out.push(finding(Severity::Error, path, None, message));
}

/// `log.md` はフロントマターを持たず（OKF v0.3 §9）、日付の見出しが `YYYY-MM-DD`。
fn check_log(path: &str, text: &str, out: &mut Vec<Finding>) {
    if text.lines().next().is_some_and(|l| l.trim_end() == "---") {
        out.push(finding(
            Severity::Error,
            path,
            None,
            "a log.md has no front matter",
        ));
    }
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
        if !entry.markdown {
            out.push(finding(
                Severity::Warning,
                index_path,
                Some(entry.line),
                format!(
                    "Markdown does not read the entry of `{}` as a link to it \
                     (remove the line and run `design-records index`, which writes a line Markdown reads)",
                    entry.url
                ),
            ));
        }
        let Some(target) = resolve(dir, &entry.url) else {
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
        // 題は表示の文字列で比べる（手で書いた対の角括弧と、index がエスケープした角括弧を同じに見る）
        let shown = plain_text(&title);
        if entry.title != shown {
            mismatches.push(format!(
                "title {:?} but the document's is {shown:?}",
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

/// 索引に既にある行の扱い。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Existing {
    /// 直さない
    Keep,
    /// 文書を指す行の説明を、文書の `description` に合わせる（足す、替える、消す）
    SyncDescriptions,
}

/// `files` の索引（`index.md`）に載っていない文書と下位のディレクトリの行を足した、索引のパスと新しい中身。
/// バンドルでなければ空。`index.md` が無いディレクトリは新しく作る。既にある行は `existing` のとおりに扱う。変わらない
/// 索引は含めない。
pub fn complete_indexes(
    files: &BTreeMap<String, String>,
    existing: Existing,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if !is_bundle(files) {
        return out;
    }
    let dirs = directories(files);
    for dir in &dirs {
        let index_path = format!("{dir}/{INDEX}");
        let current = files.get(&index_path);
        let listed = current.map(|t| listed(dir, t)).unwrap_or_default();
        let lines: Vec<String> = expected_entries(files, &dirs, dir)
            .into_iter()
            .filter(|name| !listed.contains(name))
            .map(|name| entry_line(files, dir, &name))
            .collect();
        let synced = current.map(|t| match existing {
            Existing::Keep => t.clone(),
            Existing::SyncDescriptions => sync_descriptions(files, dir, t),
        });
        if lines.is_empty() && current.is_some() && synced.as_ref() == current {
            continue;
        }
        let mut text = synced.unwrap_or_else(|| format!("# {}\n", name_of(dir)));
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

/// `dir` の索引 `index` の、文書を指す行の説明を文書の `description` に合わせた中身。Markdown がリンクと読まない行と、
/// リンクの後ろが説明の形（` - 説明`）でない行は直さない（手で書いた続きを消さない。check の注意は残る）。行の終わりの
/// 改行（CRLF を含む）は保つ。
fn sync_descriptions(files: &BTreeMap<String, String>, dir: &str, index: &str) -> String {
    let fixes: BTreeMap<usize, String> = entries(index)
        .into_iter()
        .filter(|e| e.markdown && (e.tail.trim().is_empty() || e.description.is_some()))
        .filter_map(|e| {
            let want = description_of(files.get(&resolve(dir, &e.url)?)?);
            (e.description != want.as_deref())
                .then(|| match want {
                    Some(description) => format!("{} - {description}", e.head),
                    None => e.head.to_string(),
                })
                .map(|line| (e.line, line))
        })
        .collect();
    let mut out = String::with_capacity(index.len());
    for (i, line) in index.split_inclusive('\n').enumerate() {
        match fixes.get(&(i + 1)) {
            Some(fixed) => {
                let content = line.trim_end_matches('\n').trim_end_matches('\r');
                out.push_str(fixed);
                out.push_str(&line[content.len()..]);
            }
            None => out.push_str(line),
        }
    }
    out
}

/// `dir` の索引に足す、`name`（`sub/` か `file.md`）の行。
fn entry_line(files: &BTreeMap<String, String>, dir: &str, name: &str) -> String {
    let url = percent_encode(name);
    if let Some(sub) = name.strip_suffix('/') {
        // ディレクトリの名前は Markdown でなく文字なので、文字としてエスケープする
        let mut text = String::new();
        escape_text(&mut text, sub);
        return format!("* [{text}]({url})");
    }
    let text = &files[&format!("{dir}/{name}")];
    let title = entry_title(&title_of(name, text));
    match description_of(text) {
        Some(description) => format!("* [{title}]({url}) - {description}"),
        None => format!("* [{title}]({url})"),
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
        "---\nokf_version: \"0.3\"\n---\n\n# docs\n\n* [adr](adr/) - 決定の記録\n";
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
            "---\nokf_version: \"0.3\"\n---\n\n* [adr](adr/)\n* [issues](issues/)\n",
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
    fn only_the_root_index_has_front_matter_and_a_v0_2_root_only_okf_version() {
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
            "---\nokf_version: \"0.3\"\n---\n\n# docs\n",
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
            "---\nokf_version: \"0.3\"\n---\n\n* [adr](adr/)\n* [Windows](windows.md) - 入れ方\n",
        ));
        assert_eq!(errors(&files), Vec::<String>::new());
        assert_eq!(warnings(&files), Vec::<String>::new());

        files.retain(|(p, _)| *p != "docs/index.md");
        files.push((
            "docs/index.md",
            "---\nokf_version: \"0.3\"\n---\n\n* [adr](adr/)\n* [Win](windows.md) - 別\n",
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
                "---\nokf_version: \"0.3\"\n---\n\n# docs\n",
            ),
            ("docs/adr/0001-foo.md", ADR),
            (
                "docs/windows.md",
                "---\ntype: Guide\ntitle: Windows\ndescription: 入れ方\n---\n",
            ),
        ]);
        let done = complete_indexes(&files, Existing::Keep);
        assert_eq!(
            done["docs/index.md"],
            "---\nokf_version: \"0.3\"\n---\n\n# docs\n\n\
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
        assert!(complete_indexes(&files, Existing::Keep).is_empty());
        let mut more = files.clone();
        more.insert(
            "docs/adr/0002-bar.md".to_string(),
            ADR.replace("0001: foo", "0002: bar"),
        );
        let done = complete_indexes(&more, Existing::Keep);
        assert_eq!(
            done["docs/adr/index.md"],
            "# adr\n\n* [ADR-0001: foo](0001-foo.md)\n* [ADR-0002: bar](0002-bar.md)\n"
        );
        assert!(complete_indexes(&bundle(&[("docs/a.md", "x")]), Existing::Keep).is_empty());
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
okf_version: \"0.3\"
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
