//! markdownlint --fix の直せない Markdown の違反を直すライブラリ（`claude-hooks script fix-markdown-lint` と git hook の
//! staged-checks が呼ぶ）。
//!
//! - MD060: 表の桁を揃える。全角の文字を 2 桁に数える（[`display_width`]）。最初の行の字下げ（空白とタブ）は、
//!   揃えた行に付け直す（リストの項の中の表の字下げを外すと、表が項の外に出て、その後の項が別のリストになる）。
//!   行頭の全角の空白などは Markdown の字下げでないので外す
//! - MD040: 言語の無いコードの塊の開きのフェンスに、中身から推定した言語を付ける（`guess_language`）
//!
//! フェンスは CommonMark に従い、開きのフェンスの文字（` か ~）と本数を覚え、同じ文字が同じ本数以上で後ろが空白
//! だけの行で閉じる。その間の行は、短いフェンスや別の文字のフェンスも中身で、表の桁揃えも言語の推定もしない。
//!
//! ファイルは改行を `\n` に揃えて読み（`\r\n` と `\r` も `\n` にする）、直したものだけを `\n` で書く（[`fix_file`]）。
//!
//! 振る舞いは、history-rewrite が表の揃え直しに使う fix-markdown-lint.py と同じにする（ADR-0087）。空白と幅は
//! Python の定義に合わせる（`is_py_space`、[`display_width`]）。言語の推定の正規表現の `\w`、`\s`、`\b` は Rust の
//! Unicode の定義のままで、Python と非 ASCII の一部の文字（結合文字、\x1c〜\x1f など）で食い違う。推定が外れても
//! 付く言語が変わるだけで、文書は壊れないので合わせない

use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

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
        if fence_open(lines[i]).is_some() {
            let end = fence_end(&lines, i);
            tabled.extend(
                lines[i..(end + 1).min(lines.len())]
                    .iter()
                    .map(|l| l.to_string()),
            );
            i = end + 1;
            continue;
        }
        if is_table_row(lines[i]) && i + 1 < lines.len() && is_separator_row(lines[i + 1]) {
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
        let Some((_, _, info)) = fence_open(lines[i]) else {
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

/// 表示の桁。East Asian Width が F（全角）と W（広い）の文字を 2、ほかを 1 に数え、1 文字ずつ足す。
///
/// 幅の曖昧な文字（A。→、★、● など）を 1 に数えるのは、markdownlint の MD060 が string-width で、曖昧な文字を
/// 狭く数えるため。2 に数えると、2 つのツールが桁揃えを取り合い、pre-commit の検査が通らない。結合文字、制御文字、
/// ゼロ幅の文字も 1 に数えるのは、fix-markdown-lint.py の display_width と同じ桁にするため（string-width はこれらを
/// 0 に数え、絵文字の列を 2 に数えるので、そこでは MD060 と食い違う）
pub fn display_width(s: &str) -> usize {
    s.chars().map(|c| if is_wide(c) { 2 } else { 1 }).sum()
}

fn is_wide(c: char) -> bool {
    let c = c as u32;
    WIDE.binary_search_by(|&(lo, hi)| {
        if hi < c {
            std::cmp::Ordering::Less
        } else if lo > c {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    })
    .is_ok()
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

/// 行の両端の空白と | を外し、| で切ったセル（両端の空白を外す）
fn cells(line: &str) -> Vec<&str> {
    strip(line)
        .trim_matches('|')
        .split('|')
        .map(strip)
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Align {
    Left,
    Right,
    Center,
}

/// 表の行の桁を揃える。2 行目（区切りの行）の : で寄せ方を決め、足りないセルは空のセルで埋める
pub fn format_table(table: &[&str]) -> Vec<String> {
    let indent: &str = table.first().map_or("", |l| {
        &l[..l.len() - l.trim_start_matches([' ', '\t']).len()]
    });
    let rows: Vec<Vec<&str>> = table.iter().map(|l| cells(l)).collect();
    if rows.len() < 2 {
        return table.iter().map(|l| l.to_string()).collect();
    }
    let ncols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut aligns: Vec<Align> = rows[1]
        .iter()
        .map(|c| match (c.starts_with(':'), c.ends_with(':')) {
            (true, true) => Align::Center,
            (_, true) => Align::Right,
            _ => Align::Left,
        })
        .collect();
    aligns.resize(ncols, Align::Left);

    let mut widths = vec![0usize; ncols];
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
                        Align::Left => format!(" {cell}{}", " ".repeat(pad + 1)),
                    }
                })
                .collect();
            format!("{indent}|{}|", parts.join("|"))
        })
        .collect()
}

/// 開きのフェンス（CommonMark）なら、フェンスの文字、本数、両端の空白を外した info string。` のフェンスで info
/// string に ` があれば、フェンスでない
pub fn fence_open(line: &str) -> Option<(char, usize, &str)> {
    static OPEN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^ {0,3}(`{3,}|~{3,})(.*)$").unwrap());
    let caps = OPEN.captures(line)?;
    let fence = caps.get(1)?.as_str();
    let info = caps.get(2)?.as_str();
    let ch = fence.chars().next()?;
    if ch == '`' && info.contains('`') {
        return None;
    }
    Some((ch, fence.len(), strip(info)))
}

/// line が、ch を length 本以上並べて開いたフェンスを閉じるか。閉じのフェンスは同じ文字で、本数は同じ以上で、後ろは
/// 空白だけ。短いフェンスと別の文字のフェンスは中身
fn fence_close(line: &str, ch: char, length: usize) -> bool {
    // 後ろの空白は Python の \s（str.isspace）と同じく \x1c〜\x1f を含める
    static CLOSE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^ {0,3}(`{3,}|~{3,})[\s\x1c-\x1f]*$").unwrap());
    CLOSE.captures(line).is_some_and(|caps| {
        let fence = &caps[1];
        fence.starts_with(ch) && fence.len() >= length
    })
}

/// start で開いたフェンスを閉じる行の番号。閉じなければ lines.len()
pub fn fence_end(lines: &[&str], start: usize) -> usize {
    let Some((ch, length, _)) = fence_open(lines[start]) else {
        return start;
    };
    (start + 1..lines.len())
        .find(|&j| fence_close(lines[j], ch, length))
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

/// East Asian Width が F か W の文字の範囲（両端を含む）。Unicode 16.0.0 の EastAsianWidth.txt から、次で作った
/// （Python 3.14 の unicodedata は 16.0.0。.py の display_width と同じ表にするため、版を上げるときは .py を動かす
/// Python の unicodedata.unidata_version と揃える）:
///
/// ```text
/// python3 -c 'import unicodedata as u
/// w = {c for c in range(0x110000) if u.east_asian_width(chr(c)) in ("F", "W")}
/// for a in sorted(c for c in w if c - 1 not in w):
///     b = a
///     while b + 1 in w: b += 1
///     print(f"    (0x{a:04X}, 0x{b:04X}),")'
/// ```
const WIDE: &[(u32, u32)] = &[
    (0x1100, 0x115F),
    (0x231A, 0x231B),
    (0x2329, 0x232A),
    (0x23E9, 0x23EC),
    (0x23F0, 0x23F0),
    (0x23F3, 0x23F3),
    (0x25FD, 0x25FE),
    (0x2614, 0x2615),
    (0x2630, 0x2637),
    (0x2648, 0x2653),
    (0x267F, 0x267F),
    (0x268A, 0x268F),
    (0x2693, 0x2693),
    (0x26A1, 0x26A1),
    (0x26AA, 0x26AB),
    (0x26BD, 0x26BE),
    (0x26C4, 0x26C5),
    (0x26CE, 0x26CE),
    (0x26D4, 0x26D4),
    (0x26EA, 0x26EA),
    (0x26F2, 0x26F3),
    (0x26F5, 0x26F5),
    (0x26FA, 0x26FA),
    (0x26FD, 0x26FD),
    (0x2705, 0x2705),
    (0x270A, 0x270B),
    (0x2728, 0x2728),
    (0x274C, 0x274C),
    (0x274E, 0x274E),
    (0x2753, 0x2755),
    (0x2757, 0x2757),
    (0x2795, 0x2797),
    (0x27B0, 0x27B0),
    (0x27BF, 0x27BF),
    (0x2B1B, 0x2B1C),
    (0x2B50, 0x2B50),
    (0x2B55, 0x2B55),
    (0x2E80, 0x2E99),
    (0x2E9B, 0x2EF3),
    (0x2F00, 0x2FD5),
    (0x2FF0, 0x303E),
    (0x3041, 0x3096),
    (0x3099, 0x30FF),
    (0x3105, 0x312F),
    (0x3131, 0x318E),
    (0x3190, 0x31E5),
    (0x31EF, 0x321E),
    (0x3220, 0x3247),
    (0x3250, 0xA48C),
    (0xA490, 0xA4C6),
    (0xA960, 0xA97C),
    (0xAC00, 0xD7A3),
    (0xF900, 0xFAFF),
    (0xFE10, 0xFE19),
    (0xFE30, 0xFE52),
    (0xFE54, 0xFE66),
    (0xFE68, 0xFE6B),
    (0xFF01, 0xFF60),
    (0xFFE0, 0xFFE6),
    (0x16FE0, 0x16FE4),
    (0x16FF0, 0x16FF1),
    (0x17000, 0x187F7),
    (0x18800, 0x18CD5),
    (0x18CFF, 0x18D08),
    (0x1AFF0, 0x1AFF3),
    (0x1AFF5, 0x1AFFB),
    (0x1AFFD, 0x1AFFE),
    (0x1B000, 0x1B122),
    (0x1B132, 0x1B132),
    (0x1B150, 0x1B152),
    (0x1B155, 0x1B155),
    (0x1B164, 0x1B167),
    (0x1B170, 0x1B2FB),
    (0x1D300, 0x1D356),
    (0x1D360, 0x1D376),
    (0x1F004, 0x1F004),
    (0x1F0CF, 0x1F0CF),
    (0x1F18E, 0x1F18E),
    (0x1F191, 0x1F19A),
    (0x1F200, 0x1F202),
    (0x1F210, 0x1F23B),
    (0x1F240, 0x1F248),
    (0x1F250, 0x1F251),
    (0x1F260, 0x1F265),
    (0x1F300, 0x1F320),
    (0x1F32D, 0x1F335),
    (0x1F337, 0x1F37C),
    (0x1F37E, 0x1F393),
    (0x1F3A0, 0x1F3CA),
    (0x1F3CF, 0x1F3D3),
    (0x1F3E0, 0x1F3F0),
    (0x1F3F4, 0x1F3F4),
    (0x1F3F8, 0x1F43E),
    (0x1F440, 0x1F440),
    (0x1F442, 0x1F4FC),
    (0x1F4FF, 0x1F53D),
    (0x1F54B, 0x1F54E),
    (0x1F550, 0x1F567),
    (0x1F57A, 0x1F57A),
    (0x1F595, 0x1F596),
    (0x1F5A4, 0x1F5A4),
    (0x1F5FB, 0x1F64F),
    (0x1F680, 0x1F6C5),
    (0x1F6CC, 0x1F6CC),
    (0x1F6D0, 0x1F6D2),
    (0x1F6D5, 0x1F6D7),
    (0x1F6DC, 0x1F6DF),
    (0x1F6EB, 0x1F6EC),
    (0x1F6F4, 0x1F6FC),
    (0x1F7E0, 0x1F7EB),
    (0x1F7F0, 0x1F7F0),
    (0x1F90C, 0x1F93A),
    (0x1F93C, 0x1F945),
    (0x1F947, 0x1F9FF),
    (0x1FA70, 0x1FA7C),
    (0x1FA80, 0x1FA89),
    (0x1FA8F, 0x1FAC6),
    (0x1FACE, 0x1FADC),
    (0x1FADF, 0x1FAE9),
    (0x1FAF0, 0x1FAF8),
    (0x20000, 0x2FFFD),
    (0x30000, 0x3FFFD),
];
