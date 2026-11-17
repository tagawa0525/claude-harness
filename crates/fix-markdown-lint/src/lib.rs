//! markdownlint --fix の直せない Markdown の違反を直すライブラリ（`claude-hooks script fix-markdown-lint` と git hook の
//! staged-checks が呼ぶ）。
//!
//! - MD060: 表の桁を揃える。全角の文字を 2 桁に、結合文字を 0 桁に数える（[`display_width`]）。最初の行の字下げ（空白とタブ）は、
//!   揃えた行に付け直す（リストの項の中の表の字下げを外すと、表が項の外に出て、その後の項が別のリストになる）。
//!   行頭の全角の空白などは Markdown の字下げでないので外す
//! - MD040: 言語の無いコードの塊の開きのフェンスに、中身から推定した言語を付ける（`guess_language`）
//!
//! フェンスは CommonMark に従い、開きのフェンスの文字（` か ~）と本数を覚え、同じ文字が同じ本数以上で後ろが空白
//! だけの行で閉じる。その間の行は、短いフェンスや別の文字のフェンスも中身で、表の桁揃えも言語の推定もしない。
//!
//! ファイルは改行を `\n` に揃えて読み（`\r\n` と `\r` も `\n` にする）、直したものだけを `\n` で書く（[`fix_file`]）。
//!
//! 空白は Python の定義に合わせる（`is_py_space`。ADR-0087）。言語の推定の正規表現の `\w`、
//! `\s`、`\b` は Rust の Unicode の定義のままで、Python と非 ASCII の一部の文字（結合文字、\x1c〜\x1f など）で食い違う。
//! 推定が外れても付く言語が変わるだけで、文書は壊れないので合わせない

use regex::Regex;
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
    let lines: Vec<&str> = content.split('\n').collect();

    // 表の桁を揃える。フェンスの中は飛ばす
    let mut tabled: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        if fence_open(&lines, i).is_some() {
            let end = fence_end(&lines, i);
            tabled.extend(
                lines[i..(end + 1).min(lines.len())]
                    .iter()
                    .map(|l| l.to_string()),
            );
            i = end + 1;
            continue;
        }
        if is_table_row(lines[i])
            && i + 1 < lines.len()
            && is_separator_row(lines[i + 1])
            && !is_indented_code(&lines, i)
        {
            let j = i + lines[i..].iter().take_while(|l| is_table_row(l)).count();
            tabled.extend(format_table(&lines[i..j]));
            i = j;
            continue;
        }
        tabled.push(lines[i].to_string());
        i += 1;
    }

    // 言語の無い開きのフェンスに言語を付ける。閉じのフェンスとそこまでの行はそのままコピーする
    let lines: Vec<&str> = tabled.iter().map(String::as_str).collect();
    let mut output: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let Some((_, _, info)) = fence_open(&lines, i) else {
            output.push(lines[i].to_string());
            i += 1;
            continue;
        };
        let end = fence_end(&lines, i);
        if info.is_empty() {
            let lang = guess_language(&lines[i + 1..end.min(lines.len())]);
            output.push(format!("{}{lang}", rstrip(lines[i])));
        } else {
            output.push(lines[i].to_string());
        }
        output.extend(
            lines[i + 1..(end + 1).min(lines.len())]
                .iter()
                .map(|l| l.to_string()),
        );
        i = end + 1;
    }
    output.join("\n")
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

/// 表の行（両端の空白を外すと | で始まって | で終わり、間にも | がある）
pub fn is_table_row(line: &str) -> bool {
    let s = strip(line);
    s.len() >= 2 && s.starts_with('|') && s.ends_with('|') && s[1..s.len() - 1].contains('|')
}

/// 表の区切りの行（どのセルも `-` の並びで、両端に `:` を置ける）
pub fn is_separator_row(line: &str) -> bool {
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
    let indent: &str = table.first().map_or("", |l| {
        &l[..l.len() - l.trim_start_matches([' ', '\t']).len()]
    });
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

/// 行頭の空白の桁（タブは次の 4 の倍数まで進める）
fn indent_width(line: &str) -> usize {
    let mut width = 0;
    for c in line.chars() {
        match c {
            ' ' => width += 1,
            '\t' => width += 4 - width % 4,
            _ => break,
        }
    }
    width
}

/// lines[i] が、リストの項に属さない 4 桁以上の字下げ（フェンスの無いコードブロック）か。字下げした行は、前へ
/// たどって最初の字下げの浅い行がリストの項の開き（`-`、`*`、`+`、`1.`）ならその項の中身で、そうでなければコード
pub fn is_indented_code(lines: &[&str], i: usize) -> bool {
    static MARKER: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^ {0,3}([-*+]|\d{1,9}[.)])([ \t]|$)").unwrap());
    if indent_width(lines[i]) < 4 {
        return false;
    }
    for line in lines[..i].iter().rev() {
        if strip(line).is_empty() || indent_width(line) >= 4 {
            continue;
        }
        if MARKER.is_match(line) {
            return false;
        }
        // 項の中身の続き（字下げが 1〜3 桁）は項の一部。字下げの無い行は、リストを閉じる段落
        if indent_width(line) == 0 {
            return true;
        }
    }
    true
}

/// lines[i] が開きのフェンス（CommonMark）なら、フェンスの文字、本数、両端の空白を外した info string。` のフェンスで
/// info string に ` があれば、フェンスでない。開きの字下げは 0〜3 桁で、4 桁以上はリストの項の中（項の字下げに
/// 従う）のときだけ。項に属さない 4 桁以上の字下げはコードブロックの文字
pub fn fence_open<'a>(lines: &[&'a str], i: usize) -> Option<(char, usize, &'a str)> {
    static OPEN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[ \t]*(`{3,}|~{3,})(.*)$").unwrap());
    let line = lines[i];
    let caps = OPEN.captures(line)?;
    let fence = caps.get(1)?.as_str();
    let info = caps.get(2)?.as_str();
    let ch = fence.chars().next()?;
    if ch == '`' && info.contains('`') {
        return None;
    }
    if is_indented_code(lines, i) {
        return None;
    }
    Some((ch, fence.len(), strip(info)))
}

/// line が、ch を length 本以上並べ、open_indent 桁の字下げで開いたフェンスを閉じるか。閉じのフェンスは同じ文字で、
/// 本数は同じ以上で、後ろは空白だけで、字下げは開きより 4 桁以上深くない（深ければ中身）。短いフェンスと別の文字の
/// フェンスも中身
fn fence_close(line: &str, ch: char, length: usize, open_indent: usize) -> bool {
    // 後ろの空白は Python の \s（str.isspace）と同じく \x1c〜\x1f を含める
    static CLOSE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[ \t]*(`{3,}|~{3,})[\s\x1c-\x1f]*$").unwrap());
    CLOSE.captures(line).is_some_and(|caps| {
        let fence = &caps[1];
        fence.starts_with(ch) && fence.len() >= length && indent_width(line) < open_indent + 4
    })
}

/// start で開いたフェンスを閉じる行の番号。閉じなければ lines.len()
pub fn fence_end(lines: &[&str], start: usize) -> usize {
    let Some((ch, length, _)) = fence_open(lines, start) else {
        return start;
    };
    let open_indent = indent_width(lines[start]);
    (start + 1..lines.len())
        .find(|&j| fence_close(lines[j], ch, length, open_indent))
        .unwrap_or(lines.len())
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
