//! 崩れた日本語の語の辞書（prh.yml）を読み、prh 5.4.4（textlint-rule-prh 6.0.0 が使う版）と同じ意味で文に当てる（ADR-0085）。
//!
//! 一致の意味は prh の lib/engine.js、rule.js、changeset/changeset.js、changeset/diff.js に揃える:
//!
//! - 文を `\n{2,}` で段落に分け、段落ごとにすべての規則の一致を、元の文字列の上で集める
//! - 一致した文字列が expected と同じなら直さない。expected の `$1`〜`$99` は捕捉した群に置き換える
//! - 重なった一致は、同じ開始なら長い方、違う開始なら先の方を採る
//!
//! prh の `prh:disable` の指示（paragraph.js）は、検査（[`Dictionary::check`]）でだけ当てる。textlint-rule-prh は
//! Markdown の Str のノードごとに prh を呼ぶので、指示の効く範囲はノードで決まり、全文に当てる履歴の書き換え
//! （ADR-0045。[`Dictionary::apply`]）では同じ範囲にならない。
//!
//! pattern は JavaScript の正規表現として、prh と同じ u と m のフラグで regress に読ませる。

mod autolink;
mod inline;
pub mod message;
mod pragma;
pub mod report;
pub mod strs;

use std::fmt;
use std::path::Path;

use regress::Regex;
use yaml_rust2::{Yaml, YamlLoader};

/// 辞書を読めない、prh が読めない書き方がある、specs が合わない
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryError(pub String);

/// 文に、prh と同じ意味で当てられない箇所がある
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextError(pub String);

impl fmt::Display for DictionaryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for DictionaryError {}
impl std::error::Error for TextError {}

/// 文の中の 1 つの直し（位置は文の UTF-8 のバイト）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diff {
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}

#[derive(Debug)]
struct Rule {
    expected: String,
    regex: Regex,
    /// 辞書のファイルと規則の位置（理由の表示に使う）
    origin: String,
}

/// 検査の報告の 1 件。textlint-rule-prh の報告の「actual => expected」
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// 一致の開始（文の UTF-8 のバイト）
    pub start: usize,
    pub actual: String,
    pub expected: String,
}

/// 辞書の規則の並び
#[derive(Debug)]
pub struct Dictionary {
    rules: Vec<Rule>,
}

/// prh の escapeSpecialChars が `\` を前に付ける文字（`\` 自身は含まない）
const PRH_SPECIAL: &str = "¥*+.?{}()[]^$-|/";
/// JavaScript の行の終わり（prh の regexpRegexp の `.` が当たらない文字）
const LINE_TERMINATORS: [char; 4] = ['\n', '\r', '\u{2028}', '\u{2029}'];
const RULE_KEYS: [&str; 4] = ["expected", "pattern", "patterns", "specs"];

impl Dictionary {
    /// 辞書のファイルを読む。どの規則も specs を確かめる（prh が辞書を読むときと同じ）。textlint-rule-prh は
    /// 複数の辞書を prh の Engine.merge でつなぎ、前の辞書と同じ expected の規則を外すが、辞書は 1 つだけなので
    /// 1 つのファイルだけを読む
    pub fn load(path: impl AsRef<Path>) -> Result<Self, DictionaryError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .map_err(|e| DictionaryError(format!("{}: 読めない: {e}", path.display())))?;
        Self::parse(&text, &path.display().to_string())
    }

    /// 辞書の YAML を読む。name は理由の表示に使う
    pub fn parse(yaml: &str, name: &str) -> Result<Self, DictionaryError> {
        Ok(Dictionary {
            rules: parse_rules(yaml, name)?,
        })
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// text を段落に分けて規則を当て、重なりを外した直しを先頭から並べる（prh の makeChangeSet の diffs から、
    /// prh:disable の指示で外す前のもの）。textlint-rule-prh と同じ報告は [`Dictionary::check`] が作る
    pub fn diffs(&self, text: &str) -> Result<Vec<Diff>, TextError> {
        let mut found = Vec::new();
        for (offset, paragraph) in paragraphs(text) {
            found.extend(matches(&self.rules, offset, paragraph)?);
        }
        Ok(prepare(found))
    }

    /// textlint-rule-prh が 1 つの Str の原文 text に出す報告を、出す順に並べる。段落ごとに prh:disable の指示で
    /// 一致を外してから重なりを外し（prh の makeChangeSet）、forEachChange のとおり報告する
    pub fn check(&self, text: &str) -> Result<Vec<Finding>, TextError> {
        let mut found = Vec::new();
        for (offset, paragraph) in paragraphs(text) {
            let diffs = matches(&self.rules, offset, paragraph)?;
            found.extend(pragma::Pragmas::read(paragraph, offset)?.filter(text, diffs));
        }
        Ok(report_changes(text, &prepare(found)))
    }

    /// text に規則を当てた結果
    pub fn apply(&self, text: &str) -> Result<String, TextError> {
        // prh の applyChangeSets は、当てる前にもう 1 度重なりを外す
        Ok(splice(text, &prepare(self.diffs(text)?)))
    }
}

fn parse_rules(yaml: &str, name: &str) -> Result<Vec<Rule>, DictionaryError> {
    let fail = |reason: String| DictionaryError(format!("{name}: {reason}"));
    // js-yaml と同じく、先頭の BOM を外す
    let yaml = yaml.strip_prefix('\u{feff}').unwrap_or(yaml);
    let docs = YamlLoader::load_from_str(yaml).map_err(|e| fail(format!("読めない: {e}")))?;
    if docs.len() > 1 {
        // js-yaml の load は 2 つ目の文書があれば例外にする
        return Err(fail("YAML の文書が 2 つ以上ある".into()));
    }
    let Some(Yaml::Hash(data)) = docs.into_iter().next() else {
        return Err(fail("辞書の形（version と rules）でない".into()));
    };
    let mut extra: Vec<String> = data
        .keys()
        .filter(|k| !matches!(k.as_str(), Some("version" | "rules")))
        .map(describe)
        .collect();
    if !extra.is_empty() {
        extra.sort();
        return Err(fail(format!("扱わない項目: {}", extra.join(", "))));
    }
    match data.get(&key("version")) {
        None | Some(Yaml::Integer(1)) => {}
        Some(v) => return Err(fail(format!("version は 1 だけを扱う: {}", describe(v)))),
    }
    let raw_rules = match data.get(&key("rules")) {
        None | Some(Yaml::Null) => return Ok(Vec::new()),
        Some(Yaml::Array(rules)) => rules,
        Some(_) => return Err(fail("rules が並びでない".into())),
    };
    raw_rules
        .iter()
        .enumerate()
        .map(|(i, raw)| load_rule(raw, &format!("{name} の {} 番目の規則", i + 1)))
        .collect()
}

fn key(name: &str) -> Yaml {
    Yaml::String(name.to_string())
}

/// 理由の表示に使う、YAML の値の短い書き方
fn describe(value: &Yaml) -> String {
    match value {
        Yaml::String(s) => s.clone(),
        Yaml::Integer(i) => i.to_string(),
        Yaml::Real(r) => r.clone(),
        Yaml::Boolean(b) => b.to_string(),
        other => format!("{other:?}"),
    }
}

fn load_rule(raw: &Yaml, origin: &str) -> Result<Rule, DictionaryError> {
    let fail = |reason: &str| DictionaryError(format!("{origin}: {reason}"));
    let Yaml::Hash(raw) = raw else {
        return Err(fail("pattern の無い規則（文字列だけの規則）は扱わない"));
    };
    let mut extra: Vec<String> = raw
        .keys()
        .filter(|k| !k.as_str().is_some_and(|k| RULE_KEYS.contains(&k)))
        .map(describe)
        .collect();
    if !extra.is_empty() {
        extra.sort();
        return Err(fail(&format!("扱わない項目: {}", extra.join(", "))));
    }
    let Some(Yaml::String(expected)) = raw.get(&key("expected")) else {
        return Err(fail("expected が文字列でない"));
    };
    // prh の Rule は、使わない方も含めて pattern と patterns の両方の空（空の文字列か、空の文字列を含む並び）を
    // 確かめる
    for name in ["pattern", "patterns"] {
        let empty = |v: &Yaml| v.as_str() == Some("");
        match raw.get(&key(name)) {
            Some(Yaml::Array(items)) if items.iter().any(empty) => {}
            Some(v) if empty(v) => {}
            _ => continue,
        }
        return Err(fail(&format!("{name} が空")));
    }
    // prh は `rawRule.pattern || rawRule.patterns` を使う。JavaScript では空の文字列だけが偽で、空の並びは真
    let pattern = match raw.get(&key("pattern")) {
        None | Some(Yaml::Null) => raw.get(&key("patterns")),
        some => some,
    };
    let Some(pattern) = pattern else {
        return Err(fail("pattern の無い規則は扱わない"));
    };
    let source = pattern_source(pattern, origin)?;
    let regex = Regex::with_flags(&source, "mu").map_err(|e| {
        fail(&format!(
            "/{source}/ を u フラグの正規表現として読めない: {e}"
        ))
    })?;
    let rule = Rule {
        expected: expected.clone(),
        regex,
        origin: origin.to_string(),
    };
    check_specs(&rule, raw.get(&key("specs")))?;
    Ok(rule)
}

/// prh の rule.js の _patternToRegExp に揃えて、pattern を JavaScript の正規表現の source にする
fn pattern_source(pattern: &Yaml, origin: &str) -> Result<String, DictionaryError> {
    let fail = |reason: String| DictionaryError(format!("{origin}: {reason}"));
    let pattern = match pattern {
        Yaml::Array(items) if items.is_empty() => return Err(fail("patterns が空".into())),
        Yaml::Array(items) => {
            let sources = items
                .iter()
                .map(|p| pattern_source(p, origin))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(format!("(?:{})", sources.join("|")));
        }
        Yaml::String(s) => s,
        other => {
            return Err(fail(format!(
                "pattern は文字列か文字列の並び: {}",
                describe(other)
            )));
        }
    };
    if let Some((source, flags)) = regexp_string(pattern) {
        if let Some(flag) = repeated_flag(flags) {
            return Err(fail(format!("{flag} フラグが 2 度ある")));
        }
        // g と m は prh が必ず付け、y は外すので、意味を変えない。i は辞書に無く、regress と V8 で大文字と小文字の
        // 畳み方が揃うかを確かめていないので止める（prh は並びの pattern のフラグが違えば辞書を読まない）
        if flags.contains('i') {
            return Err(fail("i フラグは扱わない".into()));
        }
        return Ok(source.to_string());
    }
    // 固定の文字列。prh は特別な文字の前に \ を付けて RegExp にする（\ 自身は付けない）。¥ と - にも付けるので、
    // u フラグの RegExp は \¥ と \- を読めず、prh も辞書を読めない
    Ok(pattern
        .chars()
        .flat_map(|c| {
            let escape = PRH_SPECIAL.contains(c).then_some('\\');
            escape.into_iter().chain(std::iter::once(c))
        })
        .collect())
}

/// prh の utils/regexp.js の regexpRegexp（`/^\/(.*)\/([gimy]*)$/`）。`.` は行の終わりに当たらず、m の無い `$` は
/// 文の終わりにだけ当たるので、行の終わりを含む文字列は固定の文字列になる
fn regexp_string(pattern: &str) -> Option<(&str, &str)> {
    if pattern.contains(LINE_TERMINATORS) {
        return None;
    }
    let body = pattern.strip_prefix('/')?;
    let (source, flags) = body.rsplit_once('/')?;
    flags
        .chars()
        .all(|c| "gimy".contains(c))
        .then_some((source, flags))
}

/// prh が `new RegExp(source, flags)` で読むときに、同じフラグが 2 度あれば SyntaxError になる
fn repeated_flag(flags: &str) -> Option<char> {
    flags
        .char_indices()
        .find(|&(i, c)| flags[..i].contains(c))
        .map(|(_, c)| c)
}

fn check_specs(rule: &Rule, specs: Option<&Yaml>) -> Result<(), DictionaryError> {
    let fail = |reason: String| DictionaryError(format!("{}: {reason}", rule.origin));
    let specs = match specs {
        None | Some(Yaml::Null) => return Ok(()),
        Some(Yaml::Array(specs)) => specs,
        Some(_) => return Err(fail("specs が並びでない".into())),
    };
    for spec in specs {
        let pair = match spec {
            Yaml::Hash(h) if h.len() == 2 => (h.get(&key("from")), h.get(&key("to"))),
            _ => (None, None),
        };
        let (Some(from), Some(to)) = pair else {
            return Err(fail(format!("specs の項目は from と to: {spec:?}")));
        };
        let (Yaml::String(from), Yaml::String(to)) = (from, to) else {
            return Err(fail(format!(
                "specs の from と to は文字列（引用符で囲む）: {spec:?}"
            )));
        };
        // prh の Rule.check は spec を段落に分けずに ChangeSet にし、applyChangeSets で当てる
        let got = matches(std::slice::from_ref(rule), 0, from)
            .map(|d| splice(from, &prepare(prepare(d))))
            .map_err(|e| DictionaryError(e.0))?;
        if &got != to {
            return Err(fail(format!(
                "specs が合わない: {from:?} は {to:?} のはずが {got:?}"
            )));
        }
    }
    Ok(())
}

/// 段落 piece（文の中の位置 offset）にすべての規則を当てた一致（prh の Paragraph の、指示で外す前の diffs）
fn matches(rules: &[Rule], offset: usize, piece: &str) -> Result<Vec<Diff>, TextError> {
    let mut found = Vec::new();
    for rule in rules {
        for m in rule.regex.find_iter(piece) {
            if m.start() == m.end() {
                // JavaScript の exec の繰り返しは、空の一致で先に進まない
                return Err(TextError(format!("{}: 空の文字列に一致する", rule.origin)));
            }
            if piece[m.range()] == rule.expected {
                continue;
            }
            let groups: Vec<Option<&str>> = m
                .captures
                .iter()
                .map(|r| r.clone().map(|r| &piece[r]))
                .collect();
            found.push(Diff {
                start: offset + m.start(),
                end: offset + m.end(),
                replacement: expand(&rule.expected, &groups),
            });
        }
    }
    Ok(found)
}

/// textlint-rule-prh の forEachChange。直しを先頭から当てながら、誤りを当てた後の文から切る。重なりは 1 度しか
/// 外していないので、前の直しと重なる一致の誤りは、前の直しの一部になる。JavaScript と同じく UTF-16 の位置で
/// 切る（UTF-8 のバイトで切ると、前の直しの中で切る位置が変わる）
fn report_changes(text: &str, diffs: &[Diff]) -> Vec<Finding> {
    // ほとんどの Str に直しは無い
    if diffs.is_empty() {
        return Vec::new();
    }
    let mut current: Vec<u16> = text.encode_utf16().collect();
    let mut delta: isize = 0;
    let mut findings = Vec::new();
    // diffs は開始の順なので、UTF-16 の位置を前から数え足す
    let (mut byte, mut unit) = (0, 0isize);
    for d in diffs {
        unit += text[byte..d.start].encode_utf16().count() as isize;
        byte = d.start;
        let length = text[d.start..d.end].encode_utf16().count() as isize;
        let expected: Vec<u16> = d.replacement.encode_utf16().collect();
        let from = unit + delta;
        let actual = js_slice(&current, from, from + length);
        if actual != expected.as_slice() {
            findings.push(Finding {
                start: d.start,
                actual: String::from_utf16_lossy(actual),
                expected: d.replacement.clone(),
            });
        }
        let end = current.len() as isize;
        current = [
            js_slice(&current, 0, from),
            &expected,
            js_slice(&current, from + length, end),
        ]
        .concat();
        delta += expected.len() as isize - length;
    }
    findings
}

/// JavaScript の String.prototype.slice。負の位置は末尾から数え、範囲の外は端に寄せる
fn js_slice(s: &[u16], start: isize, end: isize) -> &[u16] {
    let len = s.len() as isize;
    let at = |i: isize| (if i < 0 { len + i } else { i }).clamp(0, len) as usize;
    let (start, end) = (at(start), at(end));
    if start < end { &s[start..end] } else { &[] }
}

/// prh の engine.js の段落。前の区切りの後から次の `\n{2,}` までを段落にする
fn paragraphs(text: &str) -> Vec<(usize, &str)> {
    let bytes = text.as_bytes();
    let mut result = Vec::new();
    let (mut last, mut i) = (0, 0);
    while i < bytes.len() {
        if bytes[i] == b'\n' && bytes.get(i + 1) == Some(&b'\n') {
            result.push((last, &text[last..i]));
            while i < bytes.len() && bytes[i] == b'\n' {
                i += 1;
            }
            last = i;
        } else {
            i += 1;
        }
    }
    result.push((last, &text[last..]));
    result
}

/// diff.js の newText: `$N`（`/\$([0-9]{1,2})/g`）を N 番目の群に置き換える。0 と、群の数を超える N はそのまま残す
fn expand(expected: &str, groups: &[Option<&str>]) -> String {
    let mut out = String::new();
    let mut rest = expected;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let digits = after.bytes().take(2).take_while(u8::is_ascii_digit).count();
        let group = after[..digits]
            .parse::<usize>()
            .ok()
            .and_then(|n| n.checked_sub(1))
            .and_then(|i| groups.get(i));
        match group {
            Some(group) => out.push_str(group.unwrap_or("")),
            None => out.push_str(&rest[at..at + 1 + digits]),
        }
        rest = &after[digits..];
    }
    out.push_str(rest);
    out
}

/// changeset.js の _prepare。並べ、同じ開始なら長い方を、違う開始なら先の方を残す。前の要素を外す前の並びで
/// 比べるので、2 度当てると外れるものが増えうる
fn prepare(mut diffs: Vec<Diff>) -> Vec<Diff> {
    diffs.sort_by_key(|d| (d.start, d.end));
    let kept: Vec<&Diff> = diffs
        .iter()
        .enumerate()
        .filter(|&(k, d)| {
            !diffs
                .get(k + 1)
                .is_some_and(|next| d.start == next.start && d.end < next.end)
        })
        .map(|(_, d)| d)
        .collect();
    // 前の要素は、この段で外す前の並び（JavaScript の filter は元の配列を見る）
    kept.iter()
        .enumerate()
        .filter(|&(k, d)| !(k > 0 && d.start < kept[k - 1].end))
        .map(|(_, d)| (*d).clone())
        .collect()
}

fn splice(text: &str, diffs: &[Diff]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for d in diffs {
        out.push_str(&text[last..d.start]);
        out.push_str(&d.replacement);
        last = d.end;
    }
    out.push_str(&text[last..]);
    out
}
