//! markdownlint --fix の直せない Markdown の違反を直すライブラリ（`claude-hooks script fix-markdown-lint` と git hook の
//! staged-checks が呼ぶ）。
//!
//! - MD060: 表の桁を揃える。全角の文字を 2 桁に、結合文字を 0 桁に数える（[`display_width`]）。最初の行の字下げ（空白とタブ）は、
//!   揃えた行に付け直す（リストの項の中の表の字下げを外すと、表が項の外に出て、その後の項が別のリストになる）。
//!   行頭の全角の空白などは Markdown の字下げでないので外す
//! - MD040: 言語の無いコードの塊の開きのフェンスに、中身から推定した言語を付ける（`guess_language`）
//!
//! どこが表とフェンスかは、Markdown のブロックの構造（リストの項、引用、字下げしたコードブロック、フェンス、HTML の
//! ブロック、GFM の表）を pulldown-cmark で読んで決める（[`tables`]。ADR-0104）。表の揃え直しは表の範囲の行だけに、
//! 言語の付け足しは言語の無いフェンスの開きの行だけに当て、コードブロックの中身は書き換えない。
//!
//! ファイルは改行を `\n` に揃えて読み（`\r\n` と `\r` も `\n` にする）、直したものだけを `\n` で書く（[`fix_file`]）。
//!
//! 空白は Python の定義に合わせる（`is_py_space`。ADR-0087）。言語の推定の正規表現の `\w`、
//! `\s`、`\b` は Rust の Unicode の定義のままで、Python と非 ASCII の一部の文字（結合文字、\x1c〜\x1f など）で食い違う。
//! 推定が外れても付く言語が変わるだけで、文書は壊れないので合わせない

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use regex::Regex;
use std::ops::Range;
use std::path::Path;
use std::sync::LazyLock;
use unicode_width::UnicodeWidthStr;

/// path を直す。直して書いたら `Some(true)`、直すものが無ければ `Some(false)`、無ければ `None`
pub fn fix_file(path: &Path) -> Result<Option<bool>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let raw = String::from_utf8(bytes).map_err(|e| format!("UTF-8 で読めません（{e}）"))?;
    let content = raw.replace("\r\n", "\n").replace('\r', "\n");
    let fixed = fix_markdown(&content);
    if fixed == content {
        return Ok(Some(false));
    }
    std::fs::write(path, fixed).map_err(|e| e.to_string())?;
    Ok(Some(true))
}

pub fn fix_markdown(content: &str) -> String {
    let mut lines: Vec<String> = content.split('\n').map(str::to_string).collect();
    let targets = Targets::parse(content);
    for range in targets.tables {
        let rows: Vec<&str> = content
            .split('\n')
            .skip(range.start)
            .take(range.len())
            .collect();
        for (k, row) in format_table(&rows).into_iter().enumerate() {
            lines[range.start + k] = row;
        }
    }
    for fence in targets.bare_fences {
        let body: Vec<&str> = fence.body.lines().collect();
        let open = format!("{}{}", rstrip(&lines[fence.line]), guess_language(&body));
        lines[fence.line] = open;
    }
    lines.join("\n")
}

/// 揃え直す表の行の範囲（`content.split('\n')` の行の番号。半開き）。行末の `\r` は改行の一部として読む
pub fn tables(content: &str) -> Vec<Range<usize>> {
    Targets::parse(content).tables
}

/// 言語の無いフェンスの開きの行と、その中身（コンテナの字下げを外した、pulldown-cmark の読んだ中身）
struct BareFence {
    line: usize,
    body: String,
}

/// 書き換える行の位置
struct Targets {
    /// GFM の表のうち、見出しの行が表の行（[`is_table_row`]）で次の行が区切りの行のもの。範囲は見出しの行から、表の
    /// 範囲の中で、見出しと同じ字下げの表の行が続くところまで。GFM が表に数える `|` の無い行は揃えない。字下げの違う
    /// 行で止めるのは、[`format_table`] が見出しの字下げを全部の行に付け直し、行をリストの項の中や外に動かすため。
    /// pulldown-cmark は表の後ろの 4 桁以上深く字下げした行も表の行に読むが、cmark-gfm はそこで表を閉じて字下げした
    /// コードブロックを始める（ADR-0104 の確認）
    tables: Vec<Range<usize>>,
    /// 言語の無いフェンスの開き（info string が空のもの）
    bare_fences: Vec<BareFence>,
}

impl Targets {
    fn parse(content: &str) -> Targets {
        let starts: Vec<usize> = std::iter::once(0)
            .chain(content.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        let line_of = |offset: usize| starts.partition_point(|&s| s <= offset) - 1;
        let lines: Vec<&str> = content.split('\n').collect();
        let mut targets = Targets {
            tables: Vec::new(),
            bare_fences: Vec::new(),
        };
        let mut fence: Option<BareFence> = None;
        for (event, range) in Parser::new_ext(content, Options::ENABLE_TABLES).into_offset_iter() {
            match event {
                Event::Start(Tag::Table(_)) => {
                    let first = line_of(range.start);
                    let last = line_of(range.end.saturating_sub(1).max(range.start));
                    // 引用やリストの印の行の表は、行の頭が表の行でないので is_table_row が外す
                    if first < last
                        && is_table_row(lines[first])
                        && is_separator_row(lines[first + 1])
                    {
                        let indent = leading_blanks(lines[first]);
                        let end = first
                            + lines[first..=last]
                                .iter()
                                .take_while(|l| is_table_row(l) && leading_blanks(l) == indent)
                                .count();
                        if end - first >= 2 {
                            targets.tables.push(first..end);
                        }
                    }
                }
                Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info)))
                    if info.trim().is_empty() =>
                {
                    fence = Some(BareFence {
                        line: line_of(range.start),
                        body: String::new(),
                    });
                }
                Event::Text(text) => {
                    if let Some(f) = fence.as_mut() {
                        f.body.push_str(&text);
                    }
                }
                Event::End(TagEnd::CodeBlock) => {
                    targets.bare_fences.extend(fence.take());
                }
                _ => {}
            }
        }
        targets
    }
}

/// Python の str.isspace と同じ空白。Rust の char::is_whitespace（Unicode の White_Space）に、Python が空白に数える
/// \x1c〜\x1f（区切りの制御文字）を足す。str.strip と同じ文字をセルから外し、.py と同じ桁にするため
fn is_py_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

fn strip(s: &str) -> &str {
    s.trim_matches(is_py_space)
}

fn rstrip(s: &str) -> &str {
    s.trim_end_matches(is_py_space)
}

/// 表示の桁。markdownlint の MD060 が使う string-width と同じ桁に揃える（2 つのツールが桁揃えを取り合い、pre-commit の
/// 検査が通らなくなるのを避ける）。East Asian Width が F と W の文字を 2、幅の曖昧な文字（A。→、★、● など）を 1、
/// 結合文字とゼロ幅の文字を 0、絵文字の列（ZWJ の列、国旗、肌の色の修飾など）を 2 に数える。`unicode-width` が
/// これを数える。制御文字を 1 に数えるので、string-width と同じく 0 にするため、制御文字で区切って足す。
/// 2026-11-15 に全符号位置を string-width 8.3.0 と比べ、違うのは制御文字（ここで揃える）と、ハングルの中声の
/// 一部や一部の文字（インド系の文字の結合記号、U+302E と U+302F など）の 375 字（制御文字を除く）だけだった（#182）
pub fn display_width(s: &str) -> usize {
    s.split(char::is_control).map(UnicodeWidthStr::width).sum()
}

/// 行頭の字下げ（空白とタブ）
fn leading_blanks(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// 表の行（両端の空白を外すと | で始まって | で終わり、間にも | がある）
fn is_table_row(line: &str) -> bool {
    let s = strip(line);
    s.len() >= 2 && s.starts_with('|') && s.ends_with('|') && s[1..s.len() - 1].contains('|')
}

/// 表の区切りの行（どのセルも `-` の並びで、両端に `:` を置ける）
fn is_separator_row(line: &str) -> bool {
    static DASHES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^:?-+:?$").unwrap());
    if !is_table_row(line) {
        return false;
    }
    let cells: Vec<&str> = cells(line).into_iter().filter(|c| !c.is_empty()).collect();
    !cells.is_empty() && cells.iter().all(|c| DASHES.is_match(c))
}

/// 行の両端の空白と | を外し、| で切ったセル（両端の空白を外す）。`\|` はセルの中の文字で、区切りでない（GFM）
fn cells(line: &str) -> Vec<&str> {
    // 外側の | は 1 本ずつだけ外す（`\||` の閉じの前の `\|` はセルの文字）
    let line = strip(line);
    let body = line.strip_prefix('|').unwrap_or(line);
    // 末尾の | は、直前のバックスラッシュが偶数個のときだけ閉じ（奇数個なら `\|` でセルの文字）
    let body = match body.strip_suffix('|') {
        Some(rest) if (rest.len() - rest.trim_end_matches('\\').len()) % 2 == 0 => rest,
        _ => body,
    };
    let mut out = Vec::new();
    let mut start = 0;
    let mut backslashes = 0;
    for (i, b) in body.bytes().enumerate() {
        match b {
            b'\\' => backslashes += 1,
            b'|' if backslashes % 2 == 0 => {
                out.push(strip(&body[start..i]));
                start = i + 1;
                backslashes = 0;
            }
            _ => backslashes = 0,
        }
    }
    out.push(strip(&body[start..]));
    out
}

#[derive(Clone, Copy, PartialEq)]
enum Align {
    Left,
    /// 区切りの行に `:--` と書いた左寄せ（書き手の `:` を残す）
    ExplicitLeft,
    Right,
    Center,
}

impl Align {
    /// 区切りのセルが取る最小の桁（`-`、`:-`、`-:`、`:-:`）。列の幅がこれより狭いと、区切りのセルがほかの行より広くなる
    fn min_width(self) -> usize {
        match self {
            Align::Left => 1,
            Align::ExplicitLeft | Align::Right => 2,
            Align::Center => 3,
        }
    }
}

/// 表の行の桁を揃える。2 行目（区切りの行）の : で寄せ方を決め、足りないセルは空のセルで埋める。見出しより多い
/// セルの行がある表は、GFM が余分なセルを描かず markdownlint（MD056）が知らせるので、揃えずにそのまま返す
pub fn format_table(table: &[&str]) -> Vec<String> {
    let indent: &str = table.first().map_or("", |l| leading_blanks(l));
    let rows: Vec<Vec<&str>> = table.iter().map(|l| cells(l)).collect();
    if rows.len() < 2 {
        return table.iter().map(|l| l.to_string()).collect();
    }
    let ncols = rows[0].len();
    if rows.iter().any(|r| r.len() > ncols) {
        return table.iter().map(|l| l.to_string()).collect();
    }
    let mut aligns: Vec<Align> = rows[1]
        .iter()
        .map(|c| match (c.starts_with(':'), c.ends_with(':')) {
            (true, true) => Align::Center,
            (_, true) => Align::Right,
            (true, false) => Align::ExplicitLeft,
            _ => Align::Left,
        })
        .collect();
    aligns.resize(ncols, Align::Left);

    let mut widths: Vec<usize> = aligns.iter().map(|a| a.min_width()).collect();
    for (ri, row) in rows.iter().enumerate() {
        if ri == 1 {
            continue;
        }
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(display_width(cell));
        }
    }

    rows.iter()
        .enumerate()
        .map(|(ri, row)| {
            let parts: Vec<String> = (0..ncols)
                .map(|i| {
                    let w = widths[i];
                    if ri == 1 {
                        return match aligns[i] {
                            Align::Center => {
                                format!(" :{}: ", "-".repeat(w.saturating_sub(2).max(1)))
                            }
                            Align::Right => {
                                format!(" {}: ", "-".repeat(w.saturating_sub(1).max(1)))
                            }
                            Align::ExplicitLeft => {
                                format!(" :{} ", "-".repeat(w.saturating_sub(1).max(1)))
                            }
                            Align::Left => format!(" {} ", "-".repeat(w.max(1))),
                        };
                    }
                    let cell = row.get(i).copied().unwrap_or("");
                    let pad = w - display_width(cell);
                    match aligns[i] {
                        Align::Right => format!("{}{cell} ", " ".repeat(pad + 1)),
                        Align::Center => {
                            let left = pad / 2;
                            format!(
                                "{}{cell}{}",
                                " ".repeat(left + 1),
                                " ".repeat(pad - left + 1)
                            )
                        }
                        Align::Left | Align::ExplicitLeft => {
                            format!(" {cell}{}", " ".repeat(pad + 1))
                        }
                    }
                })
                .collect();
            format!("{indent}|{}|", parts.join("|"))
        })
        .collect()
}

macro_rules! found {
    ($pattern:expr, $sample:expr) => {{
        static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new($pattern).unwrap());
        RE.is_match($sample)
    }};
}

/// コードの塊の中身から言語を推定する。誤って当てないよう控えめな形だけを見て、より限られた言語から順に調べる
fn guess_language(content: &[&str]) -> &'static str {
    let s = &content.join("\n");
    if found!(
        r"(mkDerivation|buildInputs|pkgs\.|lib\.|fetchFromGitHub|stdenv|mkOption|mkEnableOption|mkIf|with pkgs|inherit \(|\.nix\b|home\.packages|programs\.\w+\.enable|nixpkgs\.overlays)",
        s
    ) {
        return "nix";
    }
    if found!(
        r"(cargo |fn \w+\(|let \w+\s*[=:]|use \w+::\w+|pub |impl \w|struct \w|enum \w|trait \w|#\[(derive|cfg|test|allow|deny|warn|must_use|inline)\]|\.unwrap\(\)|\.expect\(|-> \w+|&self|&mut )",
        s
    ) {
        return "rust";
    }
    if found!(
        r"(?m)(^def \w+\(|^class \w+|^import \w+|^from \w+ import|if __name__|\.append\(|\.items\(\)|self\.\w+|print\(|raise \w+)",
        s
    ) {
        return "python";
    }
    // JavaScript より先に調べる
    if found!(
        r#"(:\s*(string|number|boolean|void)\b|interface \w+|type \w+ =|<\w+>|as \w+|import .+ from ['"]|export (default |const |function |interface ))"#,
        s
    ) {
        return "typescript";
    }
    if found!(
        r#"(?m)(^const \w+|^let \w+|^var \w+|=>\s*\{|require\(['"]|module\.exports|console\.(log|error)|document\.|window\.)"#,
        s
    ) {
        return "javascript";
    }
    if found!(r#"^\s*\{[\s\S]*"[^"]+"\s*:"#, s) && rstrip(s).ends_with('}') {
        return "json";
    }
    if found!(r"(?m)^[\w-]+:\s+\S", s) && !found!(r"[{};]", s) {
        return "yaml";
    }
    if found!(r#"(?m)(^\[[\w.-]+\]$|^\w+\s*=\s*["'\[\{])"#, s) {
        return "toml";
    }
    if found!(r"(?i)(<html|<div|<span|<p>|<a |<img |<!DOCTYPE)", s) {
        return "html";
    }
    if found!(
        r"(?m)(^\s*\.\w+\s*\{|^\s*#\w+\s*\{|color:|font-size:|margin:|padding:)",
        s
    ) {
        return "css";
    }
    if found!(
        r"(?i)(SELECT .+ FROM|INSERT INTO|CREATE TABLE|ALTER TABLE|DROP TABLE|UPDATE .+ SET)",
        s
    ) {
        return "sql";
    }
    if found!(
        r"(?m)^(FROM |RUN |COPY |CMD |ENTRYPOINT |WORKDIR |EXPOSE |ENV )",
        s
    ) {
        return "dockerfile";
    }
    // 広く当たるので最後に調べる
    if found!(
        r"(?m)(^(git |gh |npm |pip |mkdir |cd |ls |rm |cp |mv |curl |wget |chmod |chown )|^\$ |^#!.*/bin/(ba)?sh|if \[|then$|fi$|\|\||&&)",
        s
    ) {
        return "bash";
    }
    "text"
}
