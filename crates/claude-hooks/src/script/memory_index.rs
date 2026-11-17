//! `claude-hooks script memory-index MEMORY_DIR`: メモリの索引（MEMORY.md）と実ファイル、本文の [[名前]] の参照先を
//! 突き合わせ、本文の崩れた日本語の語を辞書で検査する（`~/.claude/skills/retro/scripts/memory-index.sh` が呼ぶ）。
//!
//! 出力:
//!
//! - `ENTRIES: <索引の項目（同じディレクトリの .md へのリンク）の数>`
//! - `MISSING: <file>`: 索引にあり、ファイルが無い（索引の項目ごと）
//! - `UNINDEXED: <file>`: ファイルがあり、索引に無い（MEMORY.md 自身を除く）
//! - `BROKEN_LINK: <file> -> <名前>`: 本文の [[名前]]（[[名前|別名]]、[[名前#節]] を含む）に、語幹かフロントマターの name が
//!   <名前> のファイルが無い。コードの中（`...` と ``` か ~~~ のフェンス）と、名前の空のリンクは読まない
//! - `TERMS: <file>:<行> <誤り> => <正しい語>`: 本文（MEMORY.md を含む各 .md）の崩れた日本語の語。commit の pre-commit と
//!   同じ辞書と規則（`ja-terms check`）で見るので、コードスパン、フェンス、フロントマターの中は数えない
//!
//! ファイルは名前のバイトの順に見る。食い違いがあっても 0 で終える（判断はスキルが行う）。MEMORY.md が読めない、または
//! 辞書の検査を実行できなければ、（ほかの行を出した後に）`ERROR:` を標準エラーに出して 1 で終える。引数の誤りも 1。
//!
//! 辞書の検査は、hook と同じく claude-hooks の隣の ja-terms（[`crate::rules::ja_terms`]）。メモリは git で追跡しないので
//! pre-commit が掛からず、毎回のセッションに読み込まれて依頼やコミットにコピーされるため、ここで見る

use regex::Regex;
use std::collections::{BTreeSet, HashSet};
use std::process::{Command, Stdio};
use std::sync::LazyLock;

/// 索引のリンク先（`](name.md)`）
static INDEX_LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\]\(([^)\n]+)\)").unwrap());
/// リンクの題（`"..."`）
static LINK_TITLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"[[:space:]]+".*"$"#).unwrap());
static FENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[[:space:]]*(```|~~~)").unwrap());
static CODE_SPAN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`[^`\n]*`").unwrap());
static WIKI_LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\[([^\]\n]+)\]\]").unwrap());
/// ja-terms check の指摘の行の、ファイル名の後（`<行>:<列>: `）
static POSITION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([0-9]+):[0-9]+: ").unwrap());
/// ja-terms check の指摘の行の末尾の印（` [Error/prh]`）
static MARK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" \[[^\]]*\]$").unwrap());

pub fn main(args: &[String]) -> i32 {
    let [dir] = args else {
        eprintln!("Usage: memory-index.sh MEMORY_DIR");
        return 1;
    };
    // パスは "<dir>/<file>" の文字列で作る（ja-terms check は引数のパスのまま指摘を出すので、その頭で切る）
    let index = format!("{dir}/MEMORY.md");
    let Ok(text) = std::fs::read(&index) else {
        eprintln!("ERROR: 索引を読めません: {index}");
        return 1;
    };
    let linked = index_links(&String::from_utf8_lossy(&text));
    println!("ENTRIES: {}", linked.len());
    for target in &linked {
        if !std::path::Path::new(&format!("{dir}/{target}")).exists() {
            println!("MISSING: {target}");
        }
    }

    let files = match md_files(dir) {
        Ok(files) => files,
        Err(e) => {
            eprintln!("ERROR: メモリのディレクトリを読めません: {dir}: {e}");
            return 1;
        }
    };
    let indexed: HashSet<&str> = linked.iter().map(String::as_str).collect();
    for file in &files {
        if file != "MEMORY.md" && !indexed.contains(file.as_str()) {
            println!("UNINDEXED: {file}");
        }
    }

    // [[名前]] の参照先は、ファイル名の語幹かフロントマターの name（実際のメモリは両方を使う）
    let bodies: Vec<String> = files.iter().map(|f| read_lossy(dir, f)).collect();
    let mut known = HashSet::new();
    for (file, body) in files.iter().zip(&bodies) {
        known.insert(file.strip_suffix(".md").unwrap_or(file).to_string());
        if let Some(name) = front_matter_name(body).filter(|n| !n.is_empty()) {
            known.insert(name);
        }
    }
    for (file, body) in files.iter().zip(&bodies) {
        for name in wiki_links(body) {
            if !name.is_empty() && !known.contains(&name) {
                println!("BROKEN_LINK: {file} -> {name}");
            }
        }
    }

    terms(dir, &files)
}

/// 索引のリンク先のうち、同じディレクトリの .md。題、節（`#...`）、先頭の ./ を外す（外部の URL と別のディレクトリへの
/// 参照は索引の項目でない）。同じリンクが 2 度あれば 2 項目に数える
fn index_links(text: &str) -> Vec<String> {
    INDEX_LINK
        .captures_iter(text)
        .map(|c| {
            let target = LINK_TITLE.replace(&c[1], "");
            let target = target.split('#').next().unwrap_or_default();
            target.strip_prefix("./").unwrap_or(target).to_string()
        })
        .filter(|t| !t.contains('/') && t.ends_with(".md"))
        .collect()
}

/// ディレクトリの `*.md`（sh のグロブと同じく、. で始まる名前は除く）。名前のバイトの順
fn md_files(dir: &str) -> std::io::Result<Vec<String>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if name.ends_with(".md") && !name.starts_with('.') {
            files.push(name);
        }
    }
    files.sort();
    Ok(files)
}

/// 読めないファイル（.md の名前のディレクトリなど）は、理由を出して空の本文として扱う
fn read_lossy(dir: &str, file: &str) -> String {
    let path = format!("{dir}/{file}");
    match std::fs::read(&path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(e) => {
            eprintln!("memory-index: {path} を読めません: {e}");
            String::new()
        }
    }
}

/// 行。`\n` だけで切り、`\r` は行に残す（CRLF の `---\r` はフロントマターの区切りでない）
fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.strip_suffix('\n').unwrap_or(text).split('\n')
}

/// フロントマター（1 行目の --- から次の --- まで）の最初の `name:` の値。値の引用符（YAML の "..." と '...'）は外す
fn front_matter_name(body: &str) -> Option<String> {
    let mut lines = lines(body);
    if lines.next() != Some("---") {
        return None;
    }
    for line in lines {
        if line == "---" {
            return None;
        }
        if let Some(value) = line.strip_prefix("name:") {
            let value = value
                .trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '\x0b')
                .trim_end_matches(|c: char| c.is_ascii_whitespace() || c == '\x0b');
            let quoted = value.len() >= 2
                && ((value.starts_with('"') && value.ends_with('"'))
                    || (value.starts_with('\'') && value.ends_with('\'')));
            return Some(if quoted {
                value[1..value.len() - 1].to_string()
            } else {
                value.to_string()
            });
        }
    }
    None
}

/// 本文の [[...]] の名前（| と # より後を外し、前後の空白を外す）。フェンスの中の行とコードスパンは読まない。重複を除き、
/// 名前の順に返す
fn wiki_links(body: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut fence = false;
    for line in lines(body) {
        if FENCE.is_match(line) {
            fence = !fence;
            continue;
        }
        if fence {
            continue;
        }
        let line = CODE_SPAN.replace_all(line, "");
        for c in WIKI_LINK.captures_iter(&line) {
            let name = c[1].split(['|', '#']).next().unwrap_or_default();
            names.insert(
                name.trim_matches(|c: char| c.is_ascii_whitespace() || c == '\x0b')
                    .to_string(),
            );
        }
    }
    names
}

/// 本文の崩れた語を `ja-terms check` で見て、TERMS: の行を出す。終了コードを返す
fn terms(dir: &str, files: &[String]) -> i32 {
    // 環境変数の無い Linux は、cargo で直接ビルドしたもの（package.nix のラッパーを経ていない）
    let Some(ja_terms) = crate::rules::ja_terms() else {
        eprintln!(
            "ERROR: 辞書の検査（ja-terms）のパスがありません。Linux は claude-hooks の package.nix のラッパーが CLAUDE_HOOKS_JA_TERMS で渡します（cargo で直接ビルドしたものは、CLAUDE_HOOKS_JA_TERMS に ja-terms のパスを渡して起動する）"
        );
        return 1;
    };
    let paths: Vec<String> = files.iter().map(|f| format!("{dir}/{f}")).collect();
    let output = Command::new(&ja_terms)
        .arg("check")
        .args(&paths)
        .stderr(Stdio::inherit())
        .output();
    let output = match output {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "ERROR: 辞書の検査（{ja_terms} check）がありません。Linux は home-manager の rebuild、Windows は claude-harness の README の「Windows で使う」の cargo install で入れてください（claude-sync はバイナリを入れない）"
            );
            return 1;
        }
        Err(e) => {
            eprintln!("ERROR: 辞書の検査（{ja_terms} check）を起動できません: {e}");
            return 1;
        }
    };
    let report = String::from_utf8_lossy(&output.stdout);
    let report = report.trim_end_matches('\n');
    let found = match output.status.code() {
        Some(0) => false,
        Some(1) => true,
        code => {
            eprintln!("{report}");
            let code = code.map_or_else(|| output.status.to_string(), |c| c.to_string());
            eprintln!("ERROR: 辞書の検査（{ja_terms} check）が失敗しました（終了コード {code}）");
            return 1;
        }
    };
    let lines = term_lines(dir, files, report);
    // 崩れた語があると言いながら 1 行も読めなければ、出力の形が変わっている。黙って 0 件にしない
    if found && lines.is_empty() {
        eprintln!("{report}");
        eprintln!(
            "ERROR: 辞書の検査（{ja_terms} check）の出力を読めません（崩れた語があるが、<path>:<行>:<列>: の行が無い）"
        );
        return 1;
    }
    for line in lines {
        println!("{line}");
    }
    0
}

/// ja-terms check の「<path>:<行>:<列>: <中身> [Error/prh]」の行を、ディレクトリを外して列と印を落とした TERMS: の行に
/// する。パスは既知のファイル名との完全な前方一致で切る（名前の : で切り違えない）
fn term_lines(dir: &str, files: &[String], report: &str) -> Vec<String> {
    let prefix = format!("{dir}/");
    let mut out = Vec::new();
    for line in report.split('\n') {
        let Some(rest) = line.strip_prefix(&prefix) else {
            continue;
        };
        for file in files {
            let Some(body) = rest
                .strip_prefix(file.as_str())
                .and_then(|r| r.strip_prefix(':'))
            else {
                continue;
            };
            let Some(pos) = POSITION.captures(body) else {
                continue;
            };
            let message = MARK.replace(&body[pos[0].len()..], "");
            out.push(format!("TERMS: {file}:{} {message}", &pos[1]));
        }
    }
    out
}
