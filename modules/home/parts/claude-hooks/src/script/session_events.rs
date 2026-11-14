//! `claude-hooks script session-events [--since <日時>] [--until <日時>] [TRANSCRIPT]`: Claude Code の会話の記録から、
//! 振り返りの手がかり（拒否、失敗、ユーザーの発言）を集める（`~/.claude/skills/retro/scripts/session-events.sh` が
//! 呼ぶ。使い方、出力の形、止める入力は sh の冒頭）。
//!
//! 要約と発言の作り方は、前の jq の版と同じ出力にする（行の分け方、空白の判定、切る長さは文字の数）。jq が止まる形の
//! 行（user の行の message が object でない、発言の本文が文字列でも配列でもない、など）は、読み飛ばさずに失敗する。

use super::time::{ISO_WITH_OFFSET, parse_time, utc_millis};
use super::usage_report::{ReportError, find_transcript, glob_dir};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

const USAGE: &str = "Usage: session-events.sh [--since <日時>] [--until <日時>] [TRANSCRIPT]";

/// 記録の timestamp の形。区間の端と文字列で比べるので、この形でなければ黙って区間の外に落ちる
static RECORD_TIME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{3}Z$").unwrap()
});
static HOOK_ERROR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\S+ hook error: ").unwrap());
static PERMISSION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(Permission (for this \w+|to use .*) (was|has been) denied|The server-side auto mode classifier)")
        .unwrap()
});
static EXIT_CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new("^Exit code [0-9]+\n").unwrap());
// `failed: 0`（失敗 0 件の集計）はエラーの行に数えない。regex の crate に先読みが無いので、`failed` は FAILED で見る
static ERROR_WORD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("(?i)error|fatal|denied|cannot|not found|no such|できません|ありません|失敗")
        .unwrap()
});
static FAILED: LazyLock<Regex> = LazyLock::new(|| Regex::new("(?i)failed(: 0)?").unwrap());
static TOOL_USE_ERROR_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("</?tool_use_error>").unwrap());
static NEWLINE_SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*\n\s*").unwrap());
static COMMAND_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("<command-name>([^<]*)</command-name>").unwrap());
// `.` は改行に当たらない（jq の版の capture と同じく、複数行の引数は取らない）
static COMMAND_ARGS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("<command-args>(.*?)</command-args>").unwrap());

struct Fail(String);

fn error(message: String) -> Fail {
    Fail(format!("ERROR: {message}"))
}

fn usage() -> Fail {
    Fail(USAGE.to_string())
}

pub fn main(args: &[String]) -> i32 {
    match run(args) {
        Ok(out) => {
            print!("{out}");
            0
        }
        Err(Fail(message)) => {
            eprintln!("{message}");
            1
        }
    }
}

/// 時差つきの ISO 8601 を、記録の timestamp と同じ形の UTC（ミリ秒まで。端数は切り捨て）にする
fn to_utc(name: &str, value: &str) -> Result<String, Fail> {
    if !ISO_WITH_OFFSET.is_match(value) {
        return Err(error(format!(
            "{name} は時差つきの ISO 8601 で書いてください（例 2026-11-06T00:00+09:00、2026-11-05T15:00Z）: {value}"
        )));
    }
    let Some(t) = parse_time(value) else {
        return Err(error(format!("{name} の日時を読めません: {value}")));
    };
    Ok(utc_millis(t))
}

struct Range {
    since: Option<String>,
    until: Option<String>,
}

impl Range {
    fn given(&self) -> bool {
        self.since.is_some() || self.until.is_some()
    }

    fn contains(&self, line: &Value) -> bool {
        if !self.given() {
            return true;
        }
        let Some(ts) = line.get("timestamp").and_then(Value::as_str) else {
            return false;
        };
        self.since.as_deref().is_none_or(|s| ts >= s)
            && self.until.as_deref().is_none_or(|u| ts < u)
    }
}

fn run(args: &[String]) -> Result<String, Fail> {
    let mut range = Range {
        since: None,
        until: None,
    };
    let mut rest = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if arg == "--since" || arg == "--until" {
            let value = args.get(i + 1).ok_or_else(usage)?;
            let t = Some(to_utc(arg, value)?);
            if arg == "--since" {
                range.since = t;
            } else {
                range.until = t;
            }
            i += 2;
        } else if arg.starts_with("--") {
            return Err(usage());
        } else {
            rest.push(arg);
            i += 1;
        }
    }
    if rest.len() > 1 {
        return Err(usage());
    }
    if let (Some(s), Some(u)) = (&range.since, &range.until)
        && s >= u
    {
        return Err(error(format!(
            "--since（{s}）は --until（{u}）より前にしてください"
        )));
    }
    let mut transcript = match rest.first() {
        Some(path) => path.to_string(),
        None => find_transcript(None)
            .map_err(|ReportError(m)| error(m))?
            .display()
            .to_string(),
    };
    // jq の版は - で始まる引数をオプションに読んだので ./ を付けていた。TRANSCRIPT: の行の形を変えないため残す
    if transcript.starts_with('-') {
        transcript = format!("./{transcript}");
    }
    let main_text =
        std::fs::read(&transcript).map_err(|_| error(format!("記録を読めません: {transcript}")))?;
    let base = transcript.strip_suffix(".jsonl").unwrap_or(&transcript);
    let subagents: Vec<String> = glob_dir(&Path::new(base).join("subagents"), ".jsonl")
        .into_iter()
        .map(|p| p.display().to_string())
        .collect();

    let main = read_file(&transcript, &main_text, true, &range);
    let mut subs = Vec::new();
    for path in &subagents {
        let text = std::fs::read(path).map_err(|_| error(format!("記録を読めません: {path}")))?;
        subs.push(read_file(path, &text, false, &range));
    }
    let all = || std::iter::once(&main).chain(subs.iter());

    if main.typed == 0 {
        return Err(error(format!(
            "type を持つ行がありません（記録の形が変わったかもしれません）: {transcript}"
        )));
    }
    let uses: usize = all().map(|f| f.uses).sum();
    let results: usize = all().map(|f| f.results).sum();
    if uses > 0 && results == 0 {
        return Err(error(format!(
            "ツールの呼び出しが {uses} 件あるのに tool_result がありません（記録の形が変わったかもしれません）: {transcript}"
        )));
    }
    if range.given() {
        if let Some(bad) = all().find_map(|f| f.bad_time.clone()) {
            return Err(Fail(format!(
                "ERROR: {bad}\nERROR: 区間で絞れませんでした（上のエラーを参照）: {transcript}"
            )));
        }
        if !all().any(|f| f.in_range) {
            return Err(error(format!(
                "区間 [{}, {}) に timestamp の入る行が、本体にもサブエージェントにもありません（区間の書き誤り？）: {transcript}",
                range.since.as_deref().unwrap_or("-"),
                range.until.as_deref().unwrap_or("-")
            )));
        }
        subs.retain(|f| f.in_range);
    }
    let all = || std::iter::once(&main).chain(subs.iter());
    if let Some(bad) = all()
        .find_map(|f| f.bad_failure.clone())
        .or_else(|| main.bad_user.clone())
    {
        return Err(error(bad));
    }

    let mut out = format!("TRANSCRIPT: {transcript}\n");
    if let Some(s) = &range.since {
        out += &format!("SINCE: {s}\n");
    }
    if let Some(u) = &range.until {
        out += &format!("UNTIL: {u}\n");
    }
    out += &format!("TRANSCRIPTS: {}\n", 1 + subs.len());

    let mut counts: HashMap<(Kind, String), usize> = HashMap::new();
    for failure in all().flat_map(|f| f.failures.iter()) {
        *counts.entry(failure.clone()).or_default() += 1;
    }
    let mut counted: Vec<((Kind, String), usize)> = counts.into_iter().collect();
    // 種類の順、回数の多い順、本文のバイトの順（jq の版は sort の -k3,3 で、タブの前までを先に比べた。sort は
    // ロケールの照合順に従ったので、同じ回数の中の並びは LC_ALL=C のときと同じになる）
    counted.sort_by(|((ka, ta), ca), ((kb, tb), cb)| {
        let head = |t: &String| t.split('\t').next().unwrap_or_default().to_string();
        (ka, std::cmp::Reverse(ca), head(ta), ta).cmp(&(kb, std::cmp::Reverse(cb), head(tb), tb))
    });
    for ((kind, text), count) in counted {
        out += &format!("{}: {count} {text}\n", kind.name());
    }
    main.users
        .iter()
        .for_each(|u| out += &format!("USER: {u}\n"));
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Kind {
    Hook,
    Permission,
    Rejected,
    Tool,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Hook => "HOOK",
            Kind::Permission => "PERMISSION",
            Kind::Rejected => "REJECTED",
            Kind::Tool => "TOOL",
        }
    }
}

/// 1 つの記録から集めたもの。止める理由は、jq の版が止めた順に確かめられるよう、ここでは控えるだけにする
#[derive(Default)]
struct FileEvents {
    /// type を持つ行（本体だけ数える）
    typed: usize,
    uses: usize,
    /// user の行の tool_result（api-request-blob の行にもあるので数えない）
    results: usize,
    in_range: bool,
    /// 区間を指定したときの、形の違う最初の timestamp
    bad_time: Option<String>,
    failures: Vec<(Kind, String)>,
    bad_failure: Option<String>,
    /// ユーザーの発言（本体だけ。サブエージェントの記録の発言は親からの依頼文）
    users: Vec<String>,
    bad_user: Option<String>,
}

fn read_file(path: &str, bytes: &[u8], is_main: bool, range: &Range) -> FileEvents {
    let mut f = FileEvents::default();
    // 改行で区切った行ごとに読み、JSON の object でない行（走っている最中のセッションの書きかけの最後の行など）は飛ばす。
    // UTF-8 でないバイトは jq -R と同じく U+FFFD に読む
    for (n, raw) in bytes.split(|b| *b == b'\n').enumerate() {
        let Ok(line) = serde_json::from_str::<Value>(&String::from_utf8_lossy(raw)) else {
            continue;
        };
        if !line.is_object() {
            continue;
        }
        let at = |detail: &str| {
            format!(
                "{detail}（記録の形が変わったかもしれません）: {path}:{}",
                n + 1
            )
        };
        count_shape(&mut f, &line, is_main);
        if range.given()
            && f.bad_time.is_none()
            && let Some(ts) = line.get("timestamp")
            && !ts.as_str().is_some_and(|t| RECORD_TIME.is_match(t))
        {
            f.bad_time = Some(format!(
                "timestamp の形が違います（記録の形が変わったかもしれません）: {path}: {}",
                jq_json(ts)
            ));
        }
        if !range.contains(&line) {
            continue;
        }
        f.in_range = true;
        if f.bad_failure.is_none() {
            match failures_of(&line) {
                Ok(found) => f.failures.extend(found),
                Err(detail) => f.bad_failure = Some(at(detail)),
            }
        }
        if is_main && f.bad_user.is_none() {
            match user_message(&line) {
                Ok(Some(text)) => f.users.push(text),
                Ok(None) => {}
                Err(detail) => f.bad_user = Some(at(detail)),
            }
        }
    }
    f
}

fn is(v: &Value, key: &str, want: &str) -> bool {
    v.get(key).and_then(Value::as_str) == Some(want)
}

/// 形の検査: type を持つ行と、ツールの呼び出しと結果を数える
fn count_shape(f: &mut FileEvents, line: &Value, is_main: bool) {
    if is_main && line.get("type").is_some() {
        f.typed += 1;
    }
    let Some(content) = line
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
    else {
        return;
    };
    f.uses += content.iter().filter(|b| is(b, "type", "tool_use")).count();
    if is(line, "type", "user") {
        f.results += content
            .iter()
            .filter(|b| is(b, "type", "tool_result"))
            .count();
    }
}

/// user の行の message.content（message が無いか null なら null）
fn message_content(line: &Value) -> Result<&Value, &'static str> {
    match line.get("message") {
        None | Some(Value::Null) => Ok(&Value::Null),
        Some(Value::Object(m)) => Ok(m.get("content").unwrap_or(&Value::Null)),
        Some(_) => Err("user の行の message が object ではありません"),
    }
}

fn failures_of(line: &Value) -> Result<Vec<(Kind, String)>, &'static str> {
    if !is(line, "type", "user") {
        return Ok(Vec::new());
    }
    let Value::Array(blocks) = message_content(line)? else {
        return Ok(Vec::new());
    };
    let mut found = Vec::new();
    for block in blocks {
        match block {
            Value::Object(b) => {
                if b.get("type").and_then(Value::as_str) == Some("tool_result")
                    && b.get("is_error") == Some(&Value::Bool(true))
                {
                    found.push(classify(&body(b.get("content"))?));
                }
            }
            Value::Null => {}
            _ => return Err("message.content の要素が object ではありません"),
        }
    }
    Ok(found)
}

/// 結果の本文。配列なら各要素の text を改行でつなぐ（text の無い要素は空）
fn body(content: Option<&Value>) -> Result<String, &'static str> {
    let Some(Value::Array(blocks)) = content else {
        return Ok(match content {
            Some(Value::String(s)) => s.clone(),
            Some(v) => jq_json(v),
            None => "null".to_string(),
        });
    };
    let mut parts = Vec::new();
    for block in blocks {
        let text = block.as_object().and_then(|b| b.get("text"));
        parts.push(match text {
            Some(Value::String(s)) => s.clone(),
            None | Some(Value::Null | Value::Bool(false)) => String::new(),
            Some(Value::Bool(true)) => "true".to_string(),
            Some(n @ Value::Number(_)) => n.to_string(),
            Some(_) => return Err("ツールの結果の text が文字列ではありません"),
        });
    }
    Ok(parts.join("\n"))
}

/// jq の tojson と同じ形の JSON（キーは記録の順。jq は DEL も escape する）
fn jq_json(v: &Value) -> String {
    serde_json::to_string(v)
        .unwrap_or_default()
        .replace('\u{7f}', "\\u007f")
}

fn has_text(s: &str) -> bool {
    s.chars().any(|c| !c.is_whitespace())
}

fn first_line(s: &str) -> &str {
    s.split('\n').find(|l| has_text(l)).unwrap_or("(本文なし)")
}

fn cut(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn classify(b: &str) -> (Kind, String) {
    if HOOK_ERROR.is_match(b) {
        let rest = HOOK_ERROR.replace(b, "");
        return (Kind::Hook, cut(&normalize(first_line(&rest)), 160));
    }
    if PERMISSION.is_match(first_line(b)) {
        return (Kind::Permission, cut(first_line(b), 160));
    }
    if b.starts_with("The user doesn't want to proceed") {
        let text = match b.split_once("the user said:") {
            Some((_, said)) => format!(
                "the user said: {}",
                NEWLINE_SPACES.replace_all(said.trim_start(), " ")
            ),
            None => {
                let line = first_line(b);
                let end = "proceed with this tool use.";
                match line.find(end) {
                    Some(i) => line[..i + end.len()].to_string(),
                    None => line.to_string(),
                }
            }
        };
        return (Kind::Rejected, cut(&text, 160));
    }
    if EXIT_CODE.is_match(b) {
        let mut lines = b.split('\n');
        let head = lines.next().unwrap_or_default();
        let rest: Vec<&str> = lines.collect();
        return (
            Kind::Tool,
            cut(&format!("{head}: {}", error_line(&rest)), 160),
        );
    }
    (
        Kind::Tool,
        cut(first_line(&TOOL_USE_ERROR_TAG.replace_all(b, "")), 160),
    )
}

/// 終了コードのある失敗の出力は標準出力と標準エラーが混ざる。エラーらしい行の最後、無ければ最後の行を選ぶ
fn error_line<'a>(lines: &[&'a str]) -> &'a str {
    let lines: Vec<&str> = lines.iter().copied().filter(|l| has_text(l)).collect();
    let errorish =
        |l: &str| ERROR_WORD.is_match(l) || FAILED.captures_iter(l).any(|c| c.get(1).is_none());
    lines
        .iter()
        .copied()
        .rfind(|l| errorish(l))
        .or(lines.last().copied())
        .unwrap_or("(本文なし)")
}

/// 同じ拒否をまとめるため、コミットの番号（数字と a-f の両方を含む、英数字に接しない 7〜40 文字）を `<hash>`、
/// 英数字と `-` `#` に続かない数を `N` に置き換える
fn normalize(s: &str) -> String {
    let hex = |c: char| c.is_ascii_digit() || ('a'..='f').contains(&c);
    let chars: Vec<char> = s.chars().collect();
    let mut hashed = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        if hex(chars[i]) && (i == 0 || !chars[i - 1].is_ascii_alphanumeric()) {
            let mut j = i;
            while j < chars.len() && hex(chars[j]) {
                j += 1;
            }
            let run = &chars[i..j];
            // 列の途中から始まる番号は無い（前が英数字）ので、当たらなければ列ごと残す
            if (j == chars.len() || !chars[j].is_ascii_alphanumeric())
                && (7..=40).contains(&run.len())
                && run.iter().any(char::is_ascii_digit)
                && run.iter().any(|c| c.is_ascii_lowercase())
            {
                hashed.extend("<hash>".chars());
            } else {
                hashed.extend_from_slice(run);
            }
            i = j;
        } else {
            hashed.push(chars[i]);
            i += 1;
        }
    }
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < hashed.len() {
        let after_word =
            i > 0 && (hashed[i - 1].is_ascii_alphanumeric() || matches!(hashed[i - 1], '-' | '#'));
        if hashed[i].is_ascii_digit() && !after_word {
            while i < hashed.len() && hashed[i].is_ascii_digit() {
                i += 1;
            }
            out.push('N');
        } else {
            out.push(hashed[i]);
            i += 1;
        }
    }
    out
}

/// 印（isMeta など）が無いか、null か false
fn unflagged(line: &Value, key: &str) -> bool {
    matches!(line.get(key), None | Some(Value::Null | Value::Bool(false)))
}

/// 本体の行がユーザーの発言なら、出す形（改行を空白に、300 文字まで）
fn user_message(line: &Value) -> Result<Option<String>, &'static str> {
    let text = if is(line, "type", "user")
        && [
            "isMeta",
            "isSidechain",
            "isCompactSummary",
            "isVisibleInTranscriptOnly",
        ]
        .iter()
        .all(|k| unflagged(line, k))
    {
        match command(text_of(message_content(line)?)?) {
            Some(t) => t,
            None => return Ok(None),
        }
    } else if is(line, "type", "attachment")
        && let Some(a) = line.get("attachment").filter(|a| a.is_object())
        && is(a, "type", "queued_command")
        && or_default(a.get("commandMode"), "prompt") == Some("prompt")
        && or_default(a.get("origin").and_then(|o| o.get("kind")), "human") == Some("human")
    {
        // ターンの途中で送った発言のうち、人から届いたもの
        text_of(a.get("prompt").unwrap_or(&Value::Null))?
    } else {
        return Ok(None);
    };
    if !has_text(&text) || text.starts_with('<') {
        return Ok(None);
    }
    Ok(Some(cut(&NEWLINE_SPACES.replace_all(&text, " "), 300)))
}

/// jq の `(v // default)`: 無いか null か false なら default。文字列でなければ None（default と等しくない）
fn or_default<'a>(v: Option<&'a Value>, default: &'a str) -> Option<&'a str> {
    match v {
        None | Some(Value::Null | Value::Bool(false)) => Some(default),
        Some(v) => v.as_str(),
    }
}

/// 発言の本文。配列なら text の要素のうちタグで始まらないもの（IDE の選択、通知など）を改行でつなぐ
fn text_of(content: &Value) -> Result<String, &'static str> {
    let blocks = match content {
        Value::String(s) => return Ok(s.clone()),
        Value::Array(a) => a,
        _ => return Err("発言の本文が文字列でも配列でもありません"),
    };
    let mut texts = Vec::new();
    for block in blocks {
        match block {
            Value::Object(b) if b.get("type").and_then(Value::as_str) == Some("text") => {
                match b.get("text") {
                    Some(Value::String(t)) if !t.starts_with('<') => texts.push(t.as_str()),
                    Some(Value::String(_)) => {}
                    _ => return Err("発言の text が文字列ではありません"),
                }
            }
            Value::Object(_) | Value::Null => {}
            _ => return Err("発言の本文の要素が object ではありません"),
        }
    }
    Ok(texts.join("\n"))
}

/// スラッシュコマンドは `/<名前> <引数>` にする。名前の閉じていないものは発言にしない
fn command(text: String) -> Option<String> {
    if !text.contains("<command-name>") {
        return Some(text);
    }
    let name = COMMAND_NAME.captures(&text)?.get(1)?.as_str();
    let args = COMMAND_ARGS
        .captures(&text)
        .and_then(|c| c.get(1))
        .map_or("", |m| m.as_str());
    Some(if args.is_empty() {
        name.to_string()
    } else {
        format!("{name} {args}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_and_numbers_are_replaced_only_when_not_attached_to_words() {
        assert_eq!(normalize("push (e7c9939) 3 件"), "push (<hash>) N 件");
        assert_eq!(
            normalize("ADR-0015 #12 x12 コミットe7c9939dは約10分前"),
            "ADR-0015 #12 x12 コミット<hash>は約N分前"
        );
        // 英字の無い列は数、7 文字に満たない列と 40 文字を超える列は番号でない
        assert_eq!(normalize("10485760 1a abc1234g"), "N Na abc1234g");
        let long = "a".repeat(40) + "1";
        assert_eq!(normalize(&long), long);
    }

    #[test]
    fn the_error_line_skips_a_count_of_zero_failures() {
        assert_eq!(error_line(&["x", "passed: 1, failed: 0", "y"]), "y");
        assert_eq!(error_line(&["FAILED: 2", "y"]), "FAILED: 2");
        assert_eq!(
            error_line(&["failed: 0 then Failed", "y"]),
            "failed: 0 then Failed"
        );
        assert_eq!(error_line(&[" ", ""]), "(本文なし)");
    }
}
