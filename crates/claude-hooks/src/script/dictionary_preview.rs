//! `claude-hooks script dictionary-preview --dictionary <辞書> [--base <辞書>] [--examples <N>] [--projects <DIR>]`:
//! セッションの記録のアシスタントの発言とメモリに辞書の検査（`ja-terms check`）をかけ、辞書がどの語をどう置き換えるかを
//! 種類ごとに件数と例つきで出す（`~/.claude/scripts/dictionary-preview.sh` が呼ぶ）。辞書の規則を足す・変える PR で、
//! 置き換えた後の文を読んで、活用の崩れや別の語の一部への一致を見つけるため（specs は書いた人が思いついた例しか見ない）。
//!
//! 読むもの（`--projects` の既定は `~/.claude/projects`）:
//!
//! - `<DIR>` の下のすべての `*.jsonl`（サブエージェントの記録を含む）の、`type` が `assistant` の行の `text` の要素。
//!   行に分け、前後の空白を外し、空の行とコードのフェンス（``` か ~~~）の中を除き、同じ行は 1 回だけ数える
//! - `<DIR>/*/memory/*.md`
//!
//! 出力:
//!
//! - `SOURCES: transcripts=<記録の数> utterances=<発言の行の数> memory=<メモリのファイルの数>`
//! - `HIT: <件数> <誤り> => <正しい語>`: 発言への指摘（`--base` が無いとき）。件数の多い順、同じ件数は語の順。続く行に、
//!   字下げした例（指摘の前後 20 文字。誤りの語を【】で囲む）を、出た順に異なるものだけ `--examples` 件まで（既定は 3）
//! - `NEW:` / `GONE:`: `--base` があるとき、`--dictionary` だけにある指摘と、`--base` だけにある指摘（形は `HIT:` と同じ）。
//!   両方にある指摘は出さない
//! - `MEMORY: <project>/memory/<file>:<行> <誤り> => <正しい語>`: メモリへの `--dictionary` の指摘。メモリは毎回のセッションに
//!   読み込まれて直す対象なので、`--base` にもある指摘も出す
//!
//! 指摘があっても 0 で終える（読んで判断するのは呼んだ側）。引数の誤り、記録のディレクトリを読めない、辞書の検査を
//! 実行できないときは、`ERROR:` か使い方を標準エラーに出して 1 で終える。
//!
//! 発言は、作業用のディレクトリの `utterances.md` に、見出しの後に 1 行おきに書いて検査に渡す（Markdown として読ませ、
//! コードスパンの中を pre-commit と同じく数えない。空行で区切り、行どうしを 1 つの段落にしない）。辞書の検査は hook と
//! 同じく claude-hooks の隣の ja-terms（[`crate::rules::ja_terms`]）

use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;

const USAGE: &str = "Usage: dictionary-preview.sh --dictionary <辞書> [--base <辞書>] [--examples <N>] [--projects <DIR>]";
/// 検査に渡す発言のファイルの名前（検査は作業用のディレクトリで起動し、指摘の行の頭はこの名前になる）
const UTTERANCES: &str = "utterances.md";
/// 例に出す、指摘の前後の文字の数
const CONTEXT: usize = 20;

/// ja-terms check の指摘の行の、パスの後（`<行>:<列>: <誤り> => <正しい語>`）
static HIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([0-9]+):([0-9]+): (.*?) => (.*)$").unwrap());
/// ja-terms check の指摘の行の末尾の印（` [Error/prh]`）
static MARK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" \[[^\]]*\]$").unwrap());

struct Options {
    dictionary: String,
    base: Option<String>,
    examples: usize,
    projects: String,
}

/// 指摘の 1 件。memory は、メモリのファイルの番号（発言への指摘は None）
#[derive(Clone, PartialEq, Eq, Hash)]
struct Hit {
    memory: Option<usize>,
    line: usize,
    /// 1 から数える列（UTF-16）
    column: usize,
    from: String,
    to: String,
}

/// 作業用のディレクトリ。終わるときに消す
struct Work(PathBuf);

impl Drop for Work {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn main(args: &[String]) -> i32 {
    let Some(options) = parse(args) else {
        eprintln!("{USAGE}");
        return 1;
    };
    match run(&options) {
        Ok(out) => {
            print!("{out}");
            0
        }
        Err(message) => {
            eprintln!("ERROR: {message}");
            1
        }
    }
}

fn parse(args: &[String]) -> Option<Options> {
    let mut dictionary = None;
    let mut base = None;
    let mut examples = 3;
    let mut projects = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dictionary" => dictionary = Some(args.next()?.clone()),
            "--base" => base = Some(args.next()?.clone()),
            "--examples" => examples = args.next()?.parse().ok()?,
            "--projects" => projects = Some(args.next()?.clone()),
            _ => return None,
        }
    }
    Some(Options {
        dictionary: dictionary?,
        base,
        examples,
        projects: projects.unwrap_or_else(|| {
            crate::shell::home()
                .join(".claude/projects")
                .display()
                .to_string()
        }),
    })
}

fn run(options: &Options) -> Result<String, String> {
    let projects = &options.projects;
    let transcripts = jsonl_files(Path::new(projects))
        .map_err(|e| format!("記録のディレクトリを読めません: {projects}: {e}"))?;
    let utterances = utterances(&transcripts);
    let memory = memory_files(projects);

    let work = Work(std::env::temp_dir().join(format!(
        "claude-hooks-dictionary-preview-{}",
        std::process::id()
    )));
    std::fs::create_dir_all(&work.0)
        .and_then(|()| std::fs::write(work.0.join(UTTERANCES), document(&utterances)))
        .map_err(|e| {
            format!(
                "作業用のディレクトリに書けません: {}: {e}",
                work.0.display()
            )
        })?;

    let ja_terms = crate::rules::ja_terms().ok_or(
        "辞書の検査（ja-terms）のパスがありません。Linux は claude-hooks の package.nix のラッパーが CLAUDE_HOOKS_JA_TERMS で渡します（cargo で直接ビルドしたものは、CLAUDE_HOOKS_JA_TERMS に ja-terms のパスを渡して起動する）",
    )?;
    let paths: Vec<String> = memory.iter().map(|m| format!("{projects}/{m}")).collect();
    let new = check(&ja_terms, &work.0, &options.dictionary, &paths)?;
    let base = match &options.base {
        Some(base) => Some(check(&ja_terms, &work.0, base, &paths)?),
        None => None,
    };

    let mut out = format!(
        "SOURCES: transcripts={} utterances={} memory={}\n",
        transcripts.len(),
        utterances.len(),
        memory.len()
    );
    let spoken = |hits: &[Hit]| -> Vec<Hit> {
        hits.iter()
            .filter(|h| h.memory.is_none())
            .cloned()
            .collect()
    };
    match &base {
        None => out.push_str(&kinds("HIT", &spoken(&new), &utterances, options.examples)),
        Some(base) => {
            let only = |these: &[Hit], those: &[Hit]| -> Vec<Hit> {
                let those: HashSet<&Hit> = those.iter().collect();
                spoken(these)
                    .into_iter()
                    .filter(|h| !those.contains(h))
                    .collect()
            };
            out.push_str(&kinds(
                "NEW",
                &only(&new, base),
                &utterances,
                options.examples,
            ));
            out.push_str(&kinds(
                "GONE",
                &only(base, &new),
                &utterances,
                options.examples,
            ));
        }
    }
    for hit in &new {
        if let Some(file) = hit.memory {
            out.push_str(&format!(
                "MEMORY: {}:{} {} => {}\n",
                memory[file], hit.line, hit.from, hit.to
            ));
        }
    }
    Ok(out)
}

/// `dir` の下のすべての `*.jsonl`（`.` で始まる名前は除く）。パスの順
fn jsonl_files(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            // 読めない下のディレクトリは飛ばす（一番上が読めないときだけ失敗にする）
            found.extend(jsonl_files(&path).unwrap_or_default());
        } else if name.ends_with(".jsonl") {
            found.push(path);
        }
    }
    Ok(found)
}

/// `<projects>/*/memory/*.md` を、projects からの相対パス（`<project>/memory/<file>`）で。パスの順
fn memory_files(projects: &str) -> Vec<String> {
    let mut found = Vec::new();
    for project in names(Path::new(projects), "") {
        let dir = format!("{projects}/{project}/memory");
        for file in names(Path::new(&dir), ".md") {
            found.push(format!("{project}/memory/{file}"));
        }
    }
    found
}

/// `dir` の、`.` で始まらない名前のうち `suffix` で終わるもの。名前の順
fn names(dir: &Path, suffix: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.') && n.ends_with(suffix))
        .collect();
    found.sort();
    found
}

/// 記録のアシスタントの発言の行。読めない記録と JSON でない行は飛ばす
fn utterances(transcripts: &[PathBuf]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut lines = Vec::new();
    for transcript in transcripts {
        let Ok(bytes) = std::fs::read(transcript) else {
            continue;
        };
        for raw in String::from_utf8_lossy(&bytes).split('\n') {
            // JSON を読む前に絞る（記録の大半はツールの結果で、発言の行は一部）
            if !raw.contains("\"assistant\"") {
                continue;
            }
            let Ok(line) = serde_json::from_str::<Value>(raw) else {
                continue;
            };
            if line.get("type").and_then(Value::as_str) != Some("assistant") {
                continue;
            }
            let content = line.pointer("/message/content").and_then(Value::as_array);
            for item in content.into_iter().flatten() {
                if item.get("type").and_then(Value::as_str) != Some("text") {
                    continue;
                }
                let text = item.get("text").and_then(Value::as_str).unwrap_or_default();
                for line in prose_lines(text) {
                    if seen.insert(line.to_string()) {
                        lines.push(line.to_string());
                    }
                }
            }
        }
    }
    lines
}

/// 発言の、コードのフェンスの外の空でない行（前後の空白を外す）
fn prose_lines(text: &str) -> Vec<&str> {
    let mut fence = false;
    let mut lines = Vec::new();
    for line in text.split('\n') {
        let line = line.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            fence = !fence;
            continue;
        }
        if !fence && !line.is_empty() {
            lines.push(line);
        }
    }
    lines
}

/// 検査に渡す文書。n 番目（0 から）の発言は 2n + 3 行目（[`utterance_of`] が逆に引く）
fn document(utterances: &[String]) -> String {
    let mut text = String::from("# utterances\n");
    for line in utterances {
        text.push('\n');
        text.push_str(line);
        text.push('\n');
    }
    text
}

/// [`document`] の行（1 から）にある発言
fn utterance_of(utterances: &[String], line: usize) -> Option<&str> {
    let n = line.checked_sub(3)?;
    (n % 2 == 0)
        .then(|| utterances.get(n / 2))
        .flatten()
        .map(String::as_str)
}

/// 辞書 dictionary で発言とメモリを検査し、指摘を出た順に返す
fn check(
    ja_terms: &str,
    work: &Path,
    dictionary: &str,
    memory: &[String],
) -> Result<Vec<Hit>, String> {
    // 検査は作業用のディレクトリで起動するので、辞書のパスは絶対パスにする
    let dictionary = std::path::absolute(dictionary)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| dictionary.to_string());
    let output = Command::new(ja_terms)
        .current_dir(work)
        .args(["check", "--dictionary", &dictionary, UTTERANCES])
        .args(memory)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                format!(
                    "辞書の検査（{ja_terms} check）がありません。Linux は home-manager の rebuild、Windows は claude-harness の README の「Windows で使う」の cargo install で入れてください（claude-sync はバイナリを入れない）"
                )
            } else {
                format!("辞書の検査（{ja_terms} check）を起動できません: {e}")
            }
        })?;
    let report = String::from_utf8_lossy(&output.stdout);
    let report = report.trim_end_matches('\n');
    let found = match output.status.code() {
        Some(0) => false,
        Some(1) => true,
        code => {
            let code = code.map_or_else(|| output.status.to_string(), |c| c.to_string());
            return Err(format!(
                "辞書の検査（{ja_terms} check --dictionary {dictionary}）が失敗しました（終了コード {code}）: {report}"
            ));
        }
    };
    let hits: Vec<Hit> = report
        .split('\n')
        .filter_map(|line| hit(line, memory))
        .collect();
    // 崩れた語があると言いながら 1 行も読めなければ、出力の形が変わっている。黙って 0 件にしない
    if found && hits.is_empty() {
        return Err(format!(
            "辞書の検査（{ja_terms} check）の出力を読めません（崩れた語があるが、<path>:<行>:<列>: の行が無い）: {report}"
        ));
    }
    Ok(hits)
}

/// ja-terms check の「<path>:<行>:<列>: <誤り> => <正しい語> [Error/prh]」の行。パスは発言のファイルの名前か、渡した
/// メモリのパスとの完全な前方一致で切る（名前の : で切り違えない）
fn hit(line: &str, memory: &[String]) -> Option<Hit> {
    let line = MARK.replace(line, "");
    let after = |path: &str| line.strip_prefix(path).and_then(|r| r.strip_prefix(':'));
    let (file, rest) = match after(UTTERANCES) {
        Some(rest) => (None, rest),
        None => memory
            .iter()
            .enumerate()
            .find_map(|(n, path)| after(path).map(|rest| (Some(n), rest)))?,
    };
    let c = HIT.captures(rest)?;
    Some(Hit {
        memory: file,
        line: c[1].parse().ok()?,
        column: c[2].parse().ok()?,
        from: c[3].to_string(),
        to: c[4].to_string(),
    })
}

/// 発言への指摘を、置き換えの種類ごとに `<label>: <件数> <誤り> => <正しい語>` と例の行にする
fn kinds(label: &str, hits: &[Hit], utterances: &[String], examples: usize) -> String {
    let mut groups: BTreeMap<(&str, &str), Vec<&Hit>> = BTreeMap::new();
    for hit in hits {
        groups.entry((&hit.from, &hit.to)).or_default().push(hit);
    }
    // 件数の多い順。同じ件数は BTreeMap の語の順のまま（sort_by_key は安定）
    let mut groups: Vec<_> = groups.into_iter().collect();
    groups.sort_by_key(|group| std::cmp::Reverse(group.1.len()));
    let mut out = String::new();
    for ((from, to), hits) in groups {
        out.push_str(&format!("{label}: {} {from} => {to}\n", hits.len()));
        let mut shown: Vec<String> = Vec::new();
        for hit in hits {
            if shown.len() == examples {
                break;
            }
            let example = context(utterance_of(utterances, hit.line), hit.column, from);
            if !shown.contains(&example) {
                out.push_str(&format!("  {example}\n"));
                shown.push(example);
            }
        }
    }
    out
}

/// 指摘の前後 [`CONTEXT`] 文字。誤りの語を【】で囲む。列（UTF-16）の位置に語が無ければ（行を読めない、列の数え方が
/// 変わった）、語だけを出す
fn context(line: Option<&str>, column: usize, from: &str) -> String {
    let found = line.and_then(|line| {
        let mut units = 1;
        for (at, c) in line.char_indices() {
            if units == column {
                return line[at..].starts_with(from).then_some((line, at));
            }
            units += c.len_utf16();
        }
        None
    });
    let Some((line, at)) = found else {
        return format!("【{from}】");
    };
    let before: Vec<char> = line[..at].chars().collect();
    let before: String = before[before.len().saturating_sub(CONTEXT)..]
        .iter()
        .collect();
    let after: String = line[at + from.len()..].chars().take(CONTEXT).collect();
    format!("{before}【{from}】{after}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_document_line_of_an_utterance_maps_back_to_it() {
        let utterances = vec!["一".to_string(), "二".to_string()];
        assert_eq!(document(&utterances), "# utterances\n\n一\n\n二\n");
        assert_eq!(utterance_of(&utterances, 3), Some("一"));
        assert_eq!(utterance_of(&utterances, 5), Some("二"));
        // 見出しと空行、範囲の外
        for line in [0, 1, 2, 4, 6, 7] {
            assert_eq!(utterance_of(&utterances, line), None, "{line}");
        }
    }

    #[test]
    fn a_hit_line_is_split_at_the_known_path() {
        let memory = vec!["/p/a:b/memory/x.md".to_string()];
        let h = hit("/p/a:b/memory/x.md:6:3: 甲 => 乙 [Error/prh]", &memory).unwrap();
        assert_eq!(
            (h.memory, h.line, h.column, h.from.as_str(), h.to.as_str()),
            (Some(0), 6, 3, "甲", "乙")
        );
        // 置き換えの後ろに空白が残る規則（捕捉した空白で終わる expected）は、空白のまま読む
        let h = hit("utterances.md:3:1: 甲 1 => 乙 1  [Error/prh]", &memory).unwrap();
        assert_eq!((h.memory, h.to.as_str()), (None, "乙 1 "));
        assert!(hit("/other/x.md:1:1: 甲 => 乙 [Error/prh]", &memory).is_none());
        assert!(hit("3 problems", &memory).is_none());
    }

    #[test]
    fn the_context_falls_back_to_the_word_when_the_column_misses() {
        assert_eq!(
            context(Some("入力の甲乙です"), 4, "甲乙"),
            "入力の【甲乙】です"
        );
        assert_eq!(context(Some("入力の甲乙です"), 3, "甲乙"), "【甲乙】");
        assert_eq!(context(Some("入力"), 9, "甲乙"), "【甲乙】");
        assert_eq!(context(None, 1, "甲乙"), "【甲乙】");
    }
}
