//! `claude-hooks script usage-report [--since <日時>] [--until <日時>] [--repo <リポジトリ>]... [TRANSCRIPT]`: Claude Code の
//! 会話の記録から、区間の費用（ドル）を親・担当・レビュー・振り返りに分けて出す（ADR-0088。
//! `~/.claude/scripts/usage-report.sh` が呼ぶ。使い方と出力の形は sh の冒頭）。
//!
//! 記録の隣の `<セッション>/subagents/*.jsonl`（サブエージェントの記録）も読む。サブエージェントの区分は次で決める:
//! 最初の依頼文が `Review target:` で始まるか（/code-review が渡す依頼文）、隣の .meta.json の name が `code-review` で
//! 始まれば（/code-review が付ける名前。`code-review-2` など）レビュー。最初に読み込んだスキル（Skill ツールで呼ぶか、
//! skills/<名前>/SKILL.md を Read か Bash で読む）が retro なら振り返り。スキルを読み込んでいなければ依頼文の最初の
//! `/<スキル> を` が retro なら振り返り。ほかは担当。区間に応答の無い記録は数えない。
//!
//! 1 回の応答は本文と tool_use などで複数の行に同じ message.id と usage で記録されるので、記録ごとに message.id の
//! 2 行目からを数えない。記録の形は Claude Code の公開の仕様ではないので、価格表に無いモデル、書き込みの内訳
//! （cache_creation）の和が合わない usage、標準でない速度・地域・tier は、推測せずに exit 1。

use regex::Regex;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// ドル / 100 万トークン: (入力, 5 分の書き込み, 1 時間の書き込み, 読み出し, 出力)。
/// 出典: https://platform.claude.com/docs/en/about-claude/pricing の「Model pricing」の表（2026-11-07 に閲覧、
/// Fable 5.2 の行は 2026-11-10。ページに版と更新日の表示は無い）。読み出しの倍率はモデルで違う（同じページの注:
/// Fable 5.2 と 5.1 は入力の 0.025 倍、Opus 5.5 と Sonnet 5.5 は 0.05 倍、ほかは 0.1 倍）ので、倍率でなく表の値を持つ
const PRICES: &[(&str, Price)] = &[
    ("claude-fable-5-2", [8.0, 10.0, 16.0, 0.20, 40.0]),
    ("claude-fable-5-1", [10.0, 12.50, 20.0, 0.25, 50.0]),
    ("claude-opus-5-5", [4.0, 5.0, 8.0, 0.20, 20.0]),
    ("claude-sonnet-5-5", [2.0, 2.50, 4.0, 0.10, 10.0]),
];
/// Haiku 5.5 は 10 万トークンを超えるプロンプトで単価が上がる（同じ表の 2 行）。プロンプトの大きさは、
/// 入力、書き込み、読み出しの和で見る（キャッシュした部分もプロンプトに入るため。ページに定義の記述は無い）
const HAIKU_5_5: [Price; 2] = [
    [0.10, 0.125, 0.20, 0.01, 0.50],
    [0.50, 0.625, 1.0, 0.05, 2.50],
];
const HAIKU_5_5_LIMIT: u64 = 100_000;

type Price = [f64; 5];

const REWRITE_TOKENS: u64 = 250_000;
const KINDS: [&str; 4] = ["親", "担当", "レビュー", "振り返り"];
const TOKEN_KEYS: [&str; 5] = ["input", "write_5m", "write_1h", "read", "output"];
/// JST の時差（秒）
const JST: i64 = 9 * 3600;
const DAY_MICROS: i64 = 86_400_000_000;

static ISO_WITH_OFFSET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(:\d{2}(\.\d+)?)?(Z|[+-]\d{2}:\d{2})$").unwrap()
});
static SKILL_FILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"skills/([\w:-]+)/SKILL\.md").unwrap());
static NAMED_SKILL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/([\w:-]+) を").unwrap());

const USAGE: &str = "Usage: usage-report.sh [--since <日時>] [--until <日時>] [--repo <リポジトリ>]... [TRANSCRIPT]";

/// 時差つきの日時。比べるのは UNIX の時刻（マイクロ秒）
#[derive(Debug, Clone, Copy)]
struct Time {
    micros: i64,
}

impl PartialEq for Time {
    fn eq(&self, other: &Time) -> bool {
        self.micros == other.micros
    }
}

impl PartialOrd for Time {
    fn partial_cmp(&self, other: &Time) -> Option<std::cmp::Ordering> {
        self.micros.partial_cmp(&other.micros)
    }
}

/// 1970-01-01 からの日数（Howard Hinnant の days_from_civil。グレゴリオ暦）
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// days_from_civil の逆（年、月、日）
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        if m <= 2 {
            yoe + era * 400 + 1
        } else {
            yoe + era * 400
        },
        m,
        d,
    )
}

fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// `YYYY-MM-DDTHH:MM[:SS[.端数]](Z|±HH:MM)` を読む（Python の datetime.fromisoformat のうち、時差つきの形。秒の
/// 端数は 6 桁より後を切り捨てる）。読めなければ None
fn parse_time(s: &str) -> Option<Time> {
    let b = s.as_bytes();
    let num = |from: usize, len: usize| -> Option<i64> {
        let part = b.get(from..from + len)?;
        part.iter().all(u8::is_ascii_digit).then(|| {
            part.iter()
                .fold(0i64, |acc, d| acc * 10 + i64::from(d - b'0'))
        })
    };
    let (y, mo, d) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    if b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-') || b.get(10) != Some(&b'T') {
        return None;
    }
    let (h, mi) = (num(11, 2)?, num(14, 2)?);
    if b.get(13) != Some(&b':') {
        return None;
    }
    let mut i = 16;
    let mut sec = 0;
    let mut micros = 0;
    if b.get(i) == Some(&b':') {
        sec = num(i + 1, 2)?;
        i += 3;
        if b.get(i) == Some(&b'.') {
            let start = i + 1;
            let mut j = start;
            while b.get(j).is_some_and(u8::is_ascii_digit) {
                j += 1;
            }
            if j == start {
                return None;
            }
            let digits = &s[start..j];
            let six: String = digits
                .chars()
                .chain(std::iter::repeat('0'))
                .take(6)
                .collect();
            micros = six.parse::<i64>().ok()?;
            i = j;
        }
    }
    let offset = match &s[i..] {
        "Z" => 0,
        rest if rest.len() == 6 && (rest.starts_with('+') || rest.starts_with('-')) => {
            if b.get(i + 3) != Some(&b':') {
                return None;
            }
            let (oh, om) = (num(i + 1, 2)?, num(i + 4, 2)?);
            if oh > 23 || om > 59 {
                return None;
            }
            let sign = if rest.starts_with('-') { -1 } else { 1 };
            sign * (oh * 3600 + om * 60)
        }
        _ => return None,
    };
    if !(1..=12).contains(&mo) || d < 1 || d > days_in_month(y, mo) || h > 23 || mi > 59 || sec > 59
    {
        return None;
    }
    let seconds = days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + sec - offset;
    Some(Time {
        micros: seconds * 1_000_000 + micros,
    })
}

/// JST の暦の (年, 月, 日, 時, 分, 秒, マイクロ秒)
fn jst_fields(t: Time) -> (i64, i64, i64, i64, i64, i64, i64) {
    let local = t.micros + JST * 1_000_000;
    let days = local.div_euclid(DAY_MICROS);
    let rest = local.rem_euclid(DAY_MICROS);
    let (y, m, d) = civil_from_days(days);
    let secs = rest / 1_000_000;
    (
        y,
        m,
        d,
        secs / 3600,
        secs / 60 % 60,
        secs % 60,
        rest % 1_000_000,
    )
}

/// Python の `astimezone(JST).isoformat()`
fn jst_iso(t: Time) -> String {
    let (y, m, d, h, mi, s, us) = jst_fields(t);
    let frac = if us == 0 {
        String::new()
    } else {
        format!(".{us:06}")
    };
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}{frac}+09:00")
}

#[derive(Debug)]
struct ReportError(String);

fn fail<T>(message: String) -> Result<T, ReportError> {
    Err(ReportError(message))
}

#[derive(Default)]
struct Args {
    since: Option<String>,
    until: Option<String>,
    repos: Vec<String>,
    transcript: Option<String>,
}

/// .py の argparse と同じく、`--opt value`、`--opt=value`、一意な省略形を受ける。`--since` と `--until` は後勝ち、
/// `--repo` は重ねる。使い方の誤りは None
fn parse_args(args: &[String]) -> Option<Args> {
    let mut out = Args::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if !(arg.starts_with('-') && arg.len() > 1) {
            if out.transcript.replace(arg.clone()).is_some() {
                return None;
            }
            continue;
        }
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (arg.as_str(), None),
        };
        let matches: Vec<&str> = ["--since", "--until", "--repo"]
            .into_iter()
            .filter(|full| name.len() > 2 && full.starts_with(name))
            .collect();
        let [full] = matches[..] else { return None };
        let value = match inline {
            Some(v) => v,
            None => it
                .next()
                .filter(|v| !(v.starts_with('-') && v.len() > 1))?
                .clone(),
        };
        match full {
            "--since" => out.since = Some(value),
            "--until" => out.until = Some(value),
            _ => out.repos.push(value),
        }
    }
    Some(out)
}

pub fn main(args: &[String]) -> i32 {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return 0;
    }
    let Some(args) = parse_args(args) else {
        eprintln!("{USAGE}");
        return 2;
    };
    match report(&args) {
        Ok(()) => 0,
        Err(ReportError(message)) => {
            eprintln!("ERROR: {message}");
            1
        }
    }
}

fn parse_bound(name: &str, value: &str) -> Result<Time, ReportError> {
    match ISO_WITH_OFFSET
        .is_match(value)
        .then(|| parse_time(value))
        .flatten()
    {
        Some(t) => Ok(t),
        None => fail(format!(
            "{name} は時差つきの ISO 8601 で書いてください（例 2026-11-06T00:00+09:00、2026-11-05T15:00Z）: {value}"
        )),
    }
}

/// `dir` の、`.` で始まらない名前のうち `suffix` で終わる項目（Python の glob の `*` と同じく、隠れた名前に当てない）
fn glob_dir(dir: &Path, suffix: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            !name.starts_with('.') && name.ends_with(suffix)
        })
        .map(|e| e.path())
        .collect();
    found.sort();
    found
}

fn find_transcript(path: Option<&str>) -> Result<PathBuf, ReportError> {
    if let Some(path) = path {
        return Ok(PathBuf::from(path));
    }
    let session = std::env::var("CLAUDE_CODE_SESSION_ID").unwrap_or_default();
    if session.is_empty() {
        return fail("記録のパスを渡すか、CLAUDE_CODE_SESSION_ID を設定してください".to_string());
    }
    let projects = crate::shell::home().join(".claude").join("projects");
    let name = format!("{session}.jsonl");
    let found: Vec<PathBuf> = glob_dir(&projects, "")
        .into_iter()
        .map(|p| p.join(&name))
        .filter(|p| p.exists())
        .collect();
    match &found[..] {
        [one] => Ok(one.clone()),
        _ => fail(format!(
            "セッション {session} の記録が {} 件見つかりました（1 件のはず）",
            found.len()
        )),
    }
}

fn prices_for(model: &Value, prompt_tokens: u64) -> Result<Price, ReportError> {
    let name = model.as_str();
    if name == Some("claude-haiku-5-5") {
        return Ok(HAIKU_5_5[usize::from(prompt_tokens > HAIKU_5_5_LIMIT)]);
    }
    match PRICES.iter().find(|(n, _)| Some(*n) == name) {
        Some((_, price)) => Ok(*price),
        None => fail(format!(
            "価格表に無いモデル: {}（PRICES に価格のページの値を足してください）",
            py_str(model.clone())
        )),
    }
}

/// Python の `str(x)`（f 文字列の `{x}`）に当たる表示。None は `None`
fn py_str(v: Value) -> String {
    match v {
        Value::Null => "None".to_string(),
        Value::String(s) => s,
        other => other.to_string(),
    }
}

/// Python の `repr(x)`（f 文字列の `{x!r}`）に当たる表示
fn py_repr(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "None".to_string(),
        Some(Value::String(s)) => format!("'{s}'"),
        Some(Value::Bool(true)) => "True".to_string(),
        Some(Value::Bool(false)) => "False".to_string(),
        Some(other) => other.to_string(),
    }
}

fn text_of(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter(|x| x.get("type").and_then(Value::as_str) == Some("text"))
            .map(|x| x.get("text").and_then(Value::as_str).unwrap_or_default())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Python の真偽（`x or default` の x が偽か）
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
        Some(Value::Bool(true)) => true,
    }
}

/// 応答がスキルを Skill ツールで呼ぶか、skills/<名前>/SKILL.md を Read か Bash で読めば、その名前
fn invoked_skill(content: Option<&Value>) -> Option<String> {
    for block in content.and_then(Value::as_array)? {
        if !block.is_object() || block.get("type").and_then(Value::as_str) != Some("tool_use") {
            continue;
        }
        let input = block.get("input").filter(|v| truthy(Some(v)));
        let name = block.get("name").and_then(Value::as_str);
        let skill = input.and_then(|i| i.get("skill"));
        if name == Some("Skill") && truthy(skill) {
            return Some(
                skill
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            );
        }
        // 自動モードの担当は、ファイルを Read でなく Bash の cat や sed で読む
        let key = match name {
            Some("Read") => "file_path",
            Some("Bash") => "command",
            _ => continue,
        };
        let text = input
            .and_then(|i| i.get(key))
            .and_then(Value::as_str)
            .unwrap_or_default();
        if let Some(m) = SKILL_FILE.captures(text) {
            return Some(m[1].to_string());
        }
    }
    None
}

#[derive(Clone, Copy, Default)]
struct Tokens([u64; 5]);

impl Tokens {
    fn prompt(&self) -> u64 {
        self.0[0] + self.0[1] + self.0[2] + self.0[3]
    }
}

struct Reply {
    time: Time,
    cost: f64,
    tokens: Tokens,
    write: u64,
    rewrite_cost: f64,
}

/// usage の数（無いか null か 0 なら 0）
fn count(v: Option<&Value>) -> u64 {
    v.and_then(Value::as_u64).unwrap_or(0)
}

fn price_reply(
    path: &Path,
    model: &Value,
    u: &Value,
) -> Result<(f64, Tokens, u64, f64), ReportError> {
    let cc = u.get("cache_creation");
    let tokens = Tokens([
        count(u.get("input_tokens")),
        count(cc.and_then(|c| c.get("ephemeral_5m_input_tokens"))),
        count(cc.and_then(|c| c.get("ephemeral_1h_input_tokens"))),
        count(u.get("cache_read_input_tokens")),
        count(u.get("output_tokens")),
    ]);
    if model.as_str() == Some("<synthetic>") && tokens.0.iter().all(|&t| t == 0) {
        return Ok((0.0, tokens, 0, 0.0)); // API の失敗で Claude Code が書く応答
    }
    if tokens.0[1] + tokens.0[2] != count(u.get("cache_creation_input_tokens")) {
        return fail(format!(
            "書き込みの内訳（cache_creation）の和が cache_creation_input_tokens と合いません: {}",
            path.display()
        ));
    }
    for (key, standard) in [
        ("speed", "standard"),
        ("inference_geo", "global"),
        ("service_tier", "standard"),
    ] {
        if u.get(key).and_then(Value::as_str) != Some(standard) {
            return fail(format!(
                "usage.{key} が {} です（価格表は {standard} の値だけ）: {}",
                py_repr(u.get(key)),
                path.display()
            ));
        }
    }
    let price = prices_for(model, tokens.prompt())?;
    // .py の sum と同じ順に足す（浮動小数の和は順で変わる）
    let cost = tokens
        .0
        .iter()
        .zip(price)
        .fold(0.0, |acc, (&t, p)| acc + t as f64 * p)
        / 1e6;
    let write = tokens.0[1] + tokens.0[2];
    let rewrite_cost = (tokens.0[1] as f64 * price[1] + tokens.0[2] as f64 * price[2]) / 1e6;
    Ok((cost, tokens, write, rewrite_cost))
}

struct Transcript {
    prompt: String,
    skill: Option<String>,
    replies: Vec<Reply>,
}

fn read_transcript(
    path: &Path,
    since: Option<Time>,
    until: Option<Time>,
) -> Result<Transcript, ReportError> {
    let text = std::fs::read(path)
        .map_err(|e| ReportError(format!("記録を読めません: {}: {e}", path.display())))?;
    let text = String::from_utf8_lossy(&text);
    let mut prompt: Option<String> = None;
    let mut skill: Option<String> = None;
    let mut seen: HashSet<String> = HashSet::new();
    let mut replies = Vec::new();
    for line in text.split_inclusive('\n') {
        // 走っている最中のセッションの書きかけの最後の行
        let Ok(d) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if !d.is_object() {
            continue;
        }
        let kind = d.get("type").and_then(Value::as_str);
        if kind == Some("user") && prompt.is_none() {
            prompt = Some(text_of(d.get("message").and_then(|m| m.get("content"))));
        }
        if kind != Some("assistant") {
            continue;
        }
        let m = d.get("message").filter(|m| m.is_object());
        if skill.is_none() {
            if let Some(m) = m {
                skill = invoked_skill(m.get("content"));
            }
        }
        let Some(m) = m else { continue };
        let Some(u) = m.get("usage").filter(|u| u.is_object()) else {
            continue;
        };
        // id の無い応答は、Python の dict の get と同じく None を 1 つの id として数える
        let id = m
            .get("id")
            .map(Value::to_string)
            .unwrap_or_else(|| "null".to_string());
        if !seen.insert(id) {
            continue;
        }
        let Some(stamp) = d.get("timestamp").and_then(Value::as_str) else {
            return fail(format!(
                "timestamp の無い応答があります（記録の形が変わったかもしれません）: {}",
                path.display()
            ));
        };
        let Some(t) = parse_time(stamp) else {
            return fail(format!(
                "timestamp を時差つきの日時として読めません（記録の形が変わったかもしれません）: {}: {stamp}",
                path.display()
            ));
        };
        if since.is_some_and(|s| t < s) || until.is_some_and(|u| t >= u) {
            continue;
        }
        let model = m.get("model").cloned().unwrap_or(Value::Null);
        let (cost, tokens, write, rewrite_cost) = price_reply(path, &model, u)?;
        replies.push(Reply {
            time: t,
            cost,
            tokens,
            write,
            rewrite_cost,
        });
    }
    Ok(Transcript {
        prompt: prompt.unwrap_or_default(),
        skill,
        replies,
    })
}

fn kind_of(prompt: &str, skill: Option<String>, name: &str) -> &'static str {
    // 依頼文は親が書くので形がそろわない（「/retro を」「`retro` を Skill で」「（/retro）を行って」）。依頼文の語でなく、
    // 担当が最初に読み込んだスキルで分け、スキルを読み込まずに進めた記録だけ依頼文の「/<スキル> を」で分ける
    if prompt.starts_with("Review target:") || name.starts_with("code-review") {
        return "レビュー";
    }
    let skill = skill.or_else(|| NAMED_SKILL.captures(prompt).map(|m| m[1].to_string()));
    if skill.as_deref() == Some("retro") {
        "振り返り"
    } else {
        "担当"
    }
}

/// サブエージェントの記録の隣の .meta.json の name（/code-review が起動したものは `code-review`、`code-review-2` など）。
/// 親が自由に書く description は「/code-review の指摘を直す」のような担当にも付くので見ない。meta や name の無い記録は空
fn meta_name(path: &Path) -> Result<String, ReportError> {
    let meta = PathBuf::from(format!(
        "{}.meta.json",
        path.display().to_string().trim_end_matches(".jsonl")
    ));
    if !meta.exists() {
        return Ok(String::new());
    }
    let text = std::fs::read_to_string(&meta)
        .map_err(|e| ReportError(format!("{} を読めません: {e}", meta.display())))?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|e| ReportError(format!("{} を JSON として読めません: {e}", meta.display())))?;
    let name = value.get("name").filter(|v| truthy(Some(v)));
    Ok(name.map(|v| py_str(v.clone())).unwrap_or_default())
}

fn git_out(repo: &str, args: &[&str]) -> std::io::Result<std::process::Output> {
    std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
}

fn stderr_of(out: &std::io::Result<std::process::Output>) -> String {
    match out {
        Ok(o) => String::from_utf8_lossy(&o.stderr).trim().to_string(),
        Err(e) => e.to_string(),
    }
}

fn ok(out: &std::io::Result<std::process::Output>) -> bool {
    out.as_ref().is_ok_and(|o| o.status.success())
}

/// 主の worktree（worktree list の先頭）のブランチ。渡されたパスが別の worktree でも、既定のブランチを数えるため
fn main_branch(repo: &str) -> Result<String, ReportError> {
    let out = git_out(repo, &["worktree", "list", "--porcelain"]);
    if !ok(&out) {
        return fail(format!(
            "{repo} の worktree を読めません: {}",
            stderr_of(&out)
        ));
    }
    let stdout = String::from_utf8_lossy(&out.unwrap().stdout).into_owned();
    let first = stdout.split("\n\n").next().unwrap_or_default();
    match first
        .lines()
        .find_map(|l| l.strip_prefix("branch refs/heads/"))
    {
        Some(branch) if !branch.is_empty() => Ok(branch.to_string()),
        _ => fail(format!(
            "{repo} の主の worktree がブランチをチェックアウトしていません"
        )),
    }
}

fn count_merges(
    repo: &str,
    since: Option<Time>,
    until: Option<Time>,
) -> Result<(usize, String), ReportError> {
    let has_origin = ok(&git_out(
        repo,
        &["rev-parse", "--verify", "-q", "origin/HEAD"],
    ));
    let reference = if has_origin {
        "origin/HEAD".to_string()
    } else {
        main_branch(repo)?
    };
    let out = git_out(
        repo,
        &[
            "log",
            "--merges",
            "--first-parent",
            "--format=%cI",
            &reference,
        ],
    );
    if !ok(&out) {
        return fail(format!(
            "{repo} のマージを数えられません: {}",
            stderr_of(&out)
        ));
    }
    let stdout = String::from_utf8_lossy(&out.unwrap().stdout).into_owned();
    let mut n = 0;
    for line in stdout.split_whitespace() {
        let Some(t) = parse_time(line) else {
            return fail(format!("{repo} のコミットの日時を読めません: {line}"));
        };
        if since.is_none_or(|s| t >= s) && until.is_none_or(|u| t < u) {
            n += 1;
        }
    }
    Ok((n, reference))
}

/// Python の `f"{n:,}"`
fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Python の `" ".join(prompt.split())[:70]`
fn prompt_head(prompt: &str) -> String {
    prompt
        .split(|c: char| c.is_whitespace() || ('\x1c'..='\x1f').contains(&c))
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(70)
        .collect()
}

struct Top {
    cost: f64,
    kind: &'static str,
    replies: usize,
    context: u64,
    start: Time,
    end: Time,
    prompt: String,
}

#[derive(Default)]
struct Totals {
    cost: [f64; 4],
    count: [usize; 4],
    tokens: [[u64; 5]; 4],
    rewrites: [usize; 4],
    rewrite_cost: [f64; 4],
}

impl Totals {
    fn add(&mut self, kind: &str, replies: &[Reply]) {
        let k = KINDS.iter().position(|n| *n == kind).unwrap();
        self.count[k] += 1;
        for r in replies {
            self.cost[k] += r.cost;
            for (sum, t) in self.tokens[k].iter_mut().zip(r.tokens.0) {
                *sum += t;
            }
            if r.write > REWRITE_TOKENS {
                self.rewrites[k] += 1;
                self.rewrite_cost[k] += r.rewrite_cost;
            }
        }
    }
}

fn now_micros() -> i64 {
    let since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    since_epoch.as_micros() as i64
}

fn report(args: &Args) -> Result<(), ReportError> {
    let (since, until) = if args.since.is_none() && args.until.is_none() {
        // JST の今日の 0 時から 1 日
        let local = now_micros() + JST * 1_000_000;
        let midnight = local - local.rem_euclid(DAY_MICROS) - JST * 1_000_000;
        (
            Some(Time { micros: midnight }),
            Some(Time {
                micros: midnight + DAY_MICROS,
            }),
        )
    } else {
        let since = args
            .since
            .as_deref()
            .map(|v| parse_bound("--since", v))
            .transpose()?;
        let until = args
            .until
            .as_deref()
            .map(|v| parse_bound("--until", v))
            .transpose()?;
        if let (Some(s), Some(u)) = (since, until)
            && s >= u
        {
            return fail(format!(
                "--since（{}）は --until（{}）より前にしてください",
                args.since.as_deref().unwrap_or_default(),
                args.until.as_deref().unwrap_or_default()
            ));
        }
        (since, until)
    };
    let transcript = find_transcript(args.transcript.as_deref())?;
    if std::fs::File::open(&transcript).is_err() {
        return fail(format!("記録を読めません: {}", transcript.display()));
    }

    let mut totals = Totals::default();
    let mut top: Vec<Top> = Vec::new();
    let main = read_transcript(&transcript, since, until)?;
    totals.add("親", &main.replies);
    let stem = transcript.display().to_string();
    let subagents = Path::new(stem.strip_suffix(".jsonl").unwrap_or(&stem)).join("subagents");
    for path in glob_dir(&subagents, ".jsonl") {
        let t = read_transcript(&path, since, until)?;
        let (Some(first), Some(last)) = (t.replies.first(), t.replies.last()) else {
            continue;
        };
        let kind = kind_of(&t.prompt, t.skill.clone(), &meta_name(&path)?);
        totals.add(kind, &t.replies);
        top.push(Top {
            cost: t.replies.iter().fold(0.0, |acc, r| acc + r.cost),
            kind,
            replies: t.replies.len(),
            context: t
                .replies
                .iter()
                .map(|r| r.tokens.prompt())
                .max()
                .unwrap_or(0),
            start: first.time,
            end: last.time,
            prompt: t.prompt.clone(),
        });
    }

    let total = totals.cost.iter().fold(0.0, |acc, c| acc + c);
    println!("TRANSCRIPT: {}", transcript.display());
    if let Some(s) = since {
        println!("SINCE: {}", jst_iso(s));
    }
    if let Some(u) = until {
        println!("UNTIL: {}", jst_iso(u));
    }
    println!("TOTAL: ${total:.2}");
    for (k, name) in KINDS.iter().enumerate() {
        let share = if total != 0.0 {
            totals.cost[k] / total * 100.0
        } else {
            0.0
        };
        println!(
            "COST: {name} {} 本 ${:.2} {share:.1}% 書き直し {} 回 ${:.2}",
            totals.count[k], totals.cost[k], totals.rewrites[k], totals.rewrite_cost[k]
        );
    }
    for (k, name) in KINDS.iter().enumerate() {
        let parts: Vec<String> = TOKEN_KEYS
            .iter()
            .zip(totals.tokens[k])
            .map(|(key, n)| format!("{key}={n}"))
            .collect();
        println!("TOKENS: {name} {}", parts.join(" "));
    }
    let rewrites: usize = totals.rewrites.iter().sum();
    let rewrite_cost = totals.rewrite_cost.iter().fold(0.0, |acc, c| acc + c);
    println!("REWRITES: {rewrites} 回 ${rewrite_cost:.2}");
    let mut merges = 0;
    for repo in &args.repos {
        let (n, reference) = count_merges(repo, since, until)?;
        merges += n;
        println!("MERGES: {n} {repo} {reference}");
    }
    if args.repos.is_empty() {
        println!("PER_MERGE: -（--repo なし）");
    } else if merges == 0 {
        println!("PER_MERGE: -（マージ 0 本）");
    } else {
        println!(
            "PER_MERGE: ${:.2}（マージ {merges} 本）",
            total / merges as f64
        );
    }
    // 費用の大きい順（同じ費用は読んだ順のまま。Python の sorted の reverse=True と同じく安定）
    top.sort_by(|a, b| {
        b.cost
            .partial_cmp(&a.cost)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for t in top.iter().take(10) {
        let (_, sm, sd, sh, smi, _, _) = jst_fields(t.start);
        let (_, _, _, eh, emi, _, _) = jst_fields(t.end);
        println!(
            "TOP: ${:.2} {} {} 回 文脈の最大 {} {sm:02}-{sd:02} {sh:02}:{smi:02}-{eh:02}:{emi:02} {}",
            t.cost,
            t.kind,
            t.replies,
            thousands(t.context),
            prompt_head(&t.prompt)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_with_an_offset_are_read_as_unix_time() {
        let t = |s: &str| parse_time(s).map(|t| t.micros);
        assert_eq!(t("1970-01-01T00:00Z"), Some(0));
        assert_eq!(t("1970-01-01T09:00:00+09:00"), Some(0));
        assert_eq!(t("2026-11-06T15:00:00.000Z"), t("2026-11-07T00:00+09:00"));
        // 6 桁より後の端数は切り捨てる
        assert_eq!(t("1970-01-01T00:00:00.1234567Z"), Some(123_456));
        assert_eq!(t("2024-02-29T00:00Z").is_some(), true);
        for bad in [
            "2026-11-07T00:00",
            "2026-02-29T00:00Z",
            "2026-11-07T24:00Z",
            "2026-11-07T00:00:00.Z",
            "2026-11-07 00:00Z",
            "2026-11-07T00:00+0900",
        ] {
            assert_eq!(t(bad), None, "{bad}");
        }
    }

    #[test]
    fn jst_iso_shows_microseconds_only_when_present() {
        let t = |s: &str| jst_iso(parse_time(s).unwrap());
        assert_eq!(t("2026-11-06T15:00Z"), "2026-11-07T00:00:00+09:00");
        assert_eq!(
            t("2026-12-31T15:00:00.5Z"),
            "2027-01-01T00:00:00.500000+09:00"
        );
    }

    #[test]
    fn rounding_of_two_decimals_matches_python() {
        // Python の f"{x:.2f}" は、2 進の値のまま最も近い方へ丸める（0.125 は 0.12、0.375 は 0.38）
        let cases = [
            (0.125, "0.12"),
            (0.375, "0.38"),
            (0.005, "0.01"),
            (1.005, "1.00"),
            (0.045, "0.04"),
            (12.345, "12.35"),
        ];
        for (x, want) in cases {
            assert_eq!(format!("{x:.2}"), want, "{x}");
        }
        assert_eq!(format!("{:.1}", 0.25), "0.2");
        assert_eq!(format!("{:.1}", 0.35), "0.3");
    }

    #[test]
    fn thousands_are_separated_like_python() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(1234567), "1,234,567");
    }
}
