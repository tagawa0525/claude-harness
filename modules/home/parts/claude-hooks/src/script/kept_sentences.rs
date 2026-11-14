//! `claude-hooks script kept-sentences --old <元の版>... --new <今の版>...`: 規則の文書を縮めたとき、今の版の文のうち
//! 元の版に無いものを出す（ADR-0088。`~/.claude/scripts/kept-sentences.sh` が呼ぶ）。
//!
//! 規則の文書を縮めるときは、文を消すか、ほかの置き場を指す形に替えるだけにし、残す文の字句を変えない
//! （言い換えると、どの項や量に掛かるか（主語、否定、条件）がずれ、レビューの周ごとに食い違いが出る）。
//! 文書を塊（段落、箇条書きの項、見出し、表の行、コードブロックの行）に分け、塊を文に切って、今の版の
//! 文ごとに、元の版に同じ文があるかを見る。部分文字列でなく文の全体で比べるのは、文の頭の条件や、「。」で
//! 終わらない項目の後ろの限定を落とした文も、元の文の部分文字列になるため。文は括弧と鉤括弧の外の「。」
//! 「？」「！」と塊の終わりで切る（括弧の中で切ると、切った部分がどれも元の版にあれば、その間を消しても
//! 見つからない）。比べる前に、箇条書きの印、見出しの #、引用の > を除き、空白（行の折り返しと字下げを含む）は
//! 英数字の間だけ 1 つに縮めて残し（コマンドの中の空白の違いは見つける）、日本語の文字に接する空白を除き、
//! 文の終わりの「。」を 1 つ除く（地の文の 1 文を箇条書きの項目にしたときの「。」の有無は数えない）。
//!
//! 出力（元に無い文ごと）: `CHANGED: <今の版のファイル>:<文の始まる行>: <文>`。指す先を替えた文も出るので、出た文ごとに、
//! 指す形に替えたものか、元の文に戻すかを決める。
//!
//! 終了コード: 0 どの文も元の版にある / 1 元に無い文がある / 2 使い方の誤り、ファイルが読めない

use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

// `S` は Python の正規表現の `\s`（str.isspace の文字）。Rust の `\s`（Unicode の White_Space）は \x1c〜\x1f を含まない
static LIST_MARK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^{S}*(?:[-*+]|\d+\.){S}+")).unwrap());
static HEADING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^{S}*#{{1,6}}{S}+")).unwrap());
static QUOTE_MARK: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"^{S}*>{S}?")).unwrap());
static FENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"^{S}*(```|~~~)")).unwrap());
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"{S}+")).unwrap());
const S: &str = r"[\s\x1c-\x1f]";
const SENTENCE_END: &str = "。？！";
const OPEN: &str = "（(「『";
const CLOSE: &str = "）)」』";

const USAGE: &str = "Usage: kept-sentences.sh --old <元の版>... --new <今の版>...";

pub fn main(args: &[String]) -> i32 {
    let Some((olds, news)) = parse_args(args) else {
        eprintln!("{USAGE}");
        return 2;
    };
    // 塊ごとに切るので、別の段落の文をつないだ文は元の版のどの文とも一致しない
    let mut old = HashSet::new();
    for path in &olds {
        let Some(text) = read(path) else { return 2 };
        for block in blocks(&text) {
            old.extend(sentences(&block).into_iter().map(|(_, s)| key(&s)));
        }
    }
    let mut changed = false;
    for path in &news {
        let Some(text) = read(path) else { return 2 };
        for block in blocks(&text) {
            for (line, sentence) in sentences(&block) {
                let k = key(&sentence);
                if !k.is_empty() && !old.contains(&k) {
                    println!("CHANGED: {path}:{line}: {sentence}");
                    changed = true;
                }
            }
        }
    }
    i32::from(changed)
}

/// `--old` と `--new` の後に、次のオプションまでのファイルを 1 つ以上ずつ受ける。.py の argparse（`nargs="+"`）と同じく、
/// `--old=FILE` と一意な省略形（`--o`、`--ne` など）を受け、同じオプションを重ねると後のものだけを残す
fn parse_args(args: &[String]) -> Option<(Vec<String>, Vec<String>)> {
    let mut olds: Option<Vec<String>> = None;
    let mut news: Option<Vec<String>> = None;
    // 今ファイルを受けているオプション（true は --old）
    let mut current: Option<bool> = None;
    for arg in args {
        if arg.starts_with('-') && arg.len() > 1 {
            if let Some(is_old) = current {
                let open = if is_old { &olds } else { &news };
                if open.as_ref().is_some_and(Vec::is_empty) {
                    return None;
                }
            }
            let (name, value) = match arg.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (arg.as_str(), None),
            };
            let is_old = match name {
                _ if name.len() > 2 && "--old".starts_with(name) => true,
                _ if name.len() > 2 && "--new".starts_with(name) => false,
                _ => return None,
            };
            let slot = if is_old { &mut olds } else { &mut news };
            *slot = Some(value.map(|v| vec![v.to_string()]).unwrap_or_default());
            current = Some(is_old);
            continue;
        }
        let slot = match current? {
            true => &mut olds,
            false => &mut news,
        };
        slot.as_mut()?.push(arg.clone());
    }
    let (olds, news) = (olds?, news?);
    (!olds.is_empty() && !news.is_empty()).then_some((olds, news))
}

/// ファイルの中身。改行は .py の open の universal newlines と同じく、CRLF と CR を LF に読み替える
fn read(path: &str) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Some(text.replace("\r\n", "\n").replace('\r', "\n")),
        Err(e) => {
            eprintln!("ERROR: 読めない: {path}: {e}");
            None
        }
    }
}

/// Python の str.isspace（`str.strip` が外す文字）。Unicode の White_Space に \x1c〜\x1f を足したもの
fn is_py_space(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

fn strip(s: &str) -> &str {
    s.trim_matches(is_py_space)
}

/// 塊ごとに、(行番号, 印を除いた行) の並び
fn blocks(text: &str) -> Vec<Vec<(usize, String)>> {
    let mut out = Vec::new();
    let mut cur: Vec<(usize, String)> = Vec::new();
    let mut fence: Option<String> = None;
    for (index, raw) in text.split('\n').enumerate() {
        let no = index + 1;
        if let Some(open) = &fence {
            if FENCE.is_match(raw) && strip(raw).starts_with(open.as_str()) {
                fence = None;
            } else {
                out.push(vec![(no, raw.to_string())]);
            }
            continue;
        }
        if let Some(m) = FENCE.captures(raw) {
            flush(&mut out, &mut cur);
            fence = Some(m[1].to_string());
            continue;
        }
        if strip(raw).is_empty() {
            flush(&mut out, &mut cur);
            continue;
        }
        if HEADING.is_match(raw) || raw.trim_start_matches(is_py_space).starts_with('|') {
            flush(&mut out, &mut cur);
            out.push(vec![(no, HEADING.replace(raw, "").into_owned())]);
            continue;
        }
        let mut line = QUOTE_MARK.replace(raw, "").into_owned();
        if LIST_MARK.is_match(&line) {
            flush(&mut out, &mut cur);
            line = LIST_MARK.replace(&line, "").into_owned();
        }
        cur.push((no, line));
    }
    flush(&mut out, &mut cur);
    out
}

fn flush(out: &mut Vec<Vec<(usize, String)>>, cur: &mut Vec<(usize, String)>) {
    if !cur.is_empty() {
        out.push(std::mem::take(cur));
    }
}

/// 塊を括弧の外の文の終わりで切った、(文の始まる行番号, 文) の並び
fn sentences(block: &[(usize, String)]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut buf = String::new();
    let mut depth: usize = 0;
    for (no, line) in block {
        for ch in strip(line).chars() {
            start.get_or_insert(*no);
            buf.push(ch);
            if OPEN.contains(ch) {
                depth += 1;
            } else if CLOSE.contains(ch) {
                // 対になっていない閉じ括弧（「1)」など）で深さを負にしない
                depth = depth.saturating_sub(1);
            } else if SENTENCE_END.contains(ch) && depth == 0 {
                out.push((start.take().unwrap_or(*no), std::mem::take(&mut buf)));
            }
        }
        if buf.chars().last().is_some_and(|c| c.is_ascii()) {
            buf.push(' ');
        }
    }
    if !strip(&buf).is_empty() {
        out.push((start.unwrap_or_default(), buf));
    }
    out.into_iter()
        .map(|(no, s)| (no, strip(&s).to_string()))
        .collect()
}

/// 比べる形: 空白を 1 つに縮め、日本語の文字に接する空白を除き、文の終わりの「。」を 1 つ除く
fn key(sentence: &str) -> String {
    let collapsed = SPACES.replace_all(sentence, " ");
    let s: Vec<char> = strip(&collapsed).chars().collect();
    // 日本語の文字に接する空白は、行の折り返しの位置で有無が変わるので数えない
    let kept: String = s
        .iter()
        .enumerate()
        .filter(|&(i, &c)| {
            c != ' '
                || !(s.get(i + 1).is_some_and(|n| !n.is_ascii())
                    || i.checked_sub(1).is_some_and(|p| !s[p].is_ascii()))
        })
        .map(|(_, c)| *c)
        .collect();
    kept.strip_suffix('。').map(str::to_string).unwrap_or(kept)
}
