//! claude-hooks の結合テストのソースが、パスを `format!` で文字列に埋めるとき `Path::display()` を直に使わないこと。
//!
//! Windows の `Path::display()` は `\` で区切る。sh のコマンドに埋めると `\` が sh の解析で消え（#301）、git の書く
//! `/` 区切りのパスと比べると合わない（#304）。どちらも Linux では通るので、手元のチェックで見つけるために、
//! テストのソースを読んで調べる。パスは `common::sh_path` を通して埋める。出力が渡したパスをそのまま返すなら、
//! 渡した文字列（`to_str()`）と比べる。
//!
//! 調べるのは `format!(` の引数だけで（使い道によらない。`.expect(&format!(…))` の文も見つける）、範囲はこの crate の
//! `tests/` だけにする。`panic!` や `assert!` の引数に直に書いた失敗の文、`display().to_string()` で作った文字列、
//! `write!` などのほかのマクロ、ほかの crate のテストを調べない理由は #305 に書いた。

use std::path::{Path, PathBuf};

/// source の中で、引数に `.display()` を含む `format!` の呼び出しの行（1 から数える）
fn display_in_format(source: &str) -> Vec<usize> {
    let masked = mask_literals_and_comments(source);
    let mut lines = Vec::new();
    let mut from = 0;
    while let Some(found) = masked[from..].find("format!(") {
        let start = from + found;
        from = start + "format!(".len();
        // 識別子の一部（my_format!( など）は別のマクロ
        if masked[..start]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
        {
            continue;
        }
        let mut depth = 1;
        let mut end = from;
        for (i, c) in masked[from..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                end = from + i;
                break;
            }
        }
        if masked[from..end].contains(".display()") {
            lines.push(masked[..start].matches('\n').count() + 1);
        }
    }
    lines
}

/// 文字列と文字のリテラルの中身と、コメントを空白にする（改行は残し、行の数を変えない）。テンプレートの文字列に
/// 書いた `.display()` や `(` を、呼び出しの式と取り違えないため
fn mask_literals_and_comments(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let blank = |c: char| if c == '\n' { '\n' } else { ' ' };
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let is_ident = |at: usize| chars[at].is_alphanumeric() || chars[at] == '_';
        // br"…" の r は、b の前が識別子でなければ生の文字列の始まり
        let starts_token =
            i == 0 || !is_ident(i - 1) || (chars[i - 1] == 'b' && (i == 1 || !is_ident(i - 2)));
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                out.push(' ');
                i += 1;
            }
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            let mut depth = 0;
            while i < chars.len() {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    out.push_str("  ");
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    out.push_str("  ");
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    out.push(blank(chars[i]));
                    i += 1;
                }
            }
        } else if c == 'r'
            && starts_token
            && matches!(chars.get(i + 1), Some('"') | Some('#'))
            && raw_string_hashes(&chars, i + 1).is_some()
        {
            // r"…" と r#"…"#
            let hashes = raw_string_hashes(&chars, i + 1).unwrap();
            let open = 1 + hashes + 1;
            out.push('r');
            out.extend(std::iter::repeat_n(' ', open - 1));
            i += open;
            while i < chars.len() {
                if chars[i] == '"' && (1..=hashes).all(|k| chars.get(i + k) == Some(&'#')) {
                    out.extend(std::iter::repeat_n(' ', 1 + hashes));
                    i += 1 + hashes;
                    break;
                }
                out.push(blank(chars[i]));
                i += 1;
            }
        } else if c == '"' {
            out.push(' ');
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' {
                    out.push(' ');
                    i += 1;
                }
                if i < chars.len() {
                    out.push(blank(chars[i]));
                    i += 1;
                }
            }
            out.push(' ');
            i += 1;
        } else if c == '\'' && chars.get(i + 1) == Some(&'\\') {
            // '\n' や '\'' などのエスケープの文字。\ の次の 1 文字は、' でも終わりにしない
            out.push_str("   ");
            i += 3;
            while i < chars.len() && chars[i] != '\'' {
                out.push(' ');
                i += 1;
            }
            out.push(' ');
            i += 1;
        } else if c == '\'' && chars.get(i + 2) == Some(&'\'') {
            // 'x' の文字（'a のライフタイムは、2 つ後が ' でないので通す）
            out.push_str("   ");
            i += 3;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// chars[at] から始まる `#…#"` の `#` の数。生の文字列の始まりでなければ None
fn raw_string_hashes(chars: &[char], at: usize) -> Option<usize> {
    let hashes = chars[at..].iter().take_while(|&&c| c == '#').count();
    (chars.get(at + hashes) == Some(&'"')).then_some(hashes)
}

fn rust_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
}

#[test]
fn a_displayed_path_embedded_in_a_command_is_found() {
    // #301 で直す前の rule_pre_pr_create_check.rs（3d5f0279^）
    let source = r#"
        let command = format!(
            "cd {} && {CREATE}; cd {} && {CREATE}",
            first.display(),
            second.display()
        );
        let out = repo.hook(&command);
"#;
    assert_eq!(display_in_format(source), [2]);
}

#[test]
fn a_displayed_path_in_an_expected_output_is_found() {
    // #304 で直す前の script_resume_state.rs（b5e18ec4^）
    let source = r#"
    assert_eq!(
        section(&text, "worktrees"),
        format!(
            "{} main session=sess-1\n{} feat/x dirty session=sess-1\n",
            repo.app().display(),
            wt.display()
        )
    );
    assert_eq!(
        section(&text, "worktrees"),
        format!("{} main\n", repo.app().display())
    );
"#;
    assert_eq!(display_in_format(source), [4, 12]);
}

#[test]
fn paths_through_sh_path_are_not_found() {
    // #301 と #304 で直した後の形
    let source = r#"
        let command = format!(
            "cd {} && {CREATE}; cd {} && {CREATE}",
            common::sh_path(first),
            common::sh_path(second)
        );
        format!("{} main\n", common::sh_path(&repo.app()))
"#;
    assert_eq!(display_in_format(source), [] as [usize; 0]);
}

#[test]
fn messages_comments_and_literals_are_not_found() {
    let source = r###"
        panic!("{} が無い", path.display());
        assert!(ok, "{}", path.display());
        let arg = path.display().to_string();
        // format!("{}", path.display())
        /* format!("{}", path.display()) */
        let text = "format!(\"{}\", path.display())";
        let raw = r##"format!("{}", path.display())"##;
        let paren = format!("{}{}", ')', name);
        let quote = format!("{}{}", '\'', name);
        let after = path.display();
        fn f<'a>(x: &'a str) -> String { format!("{x}") }
        let bytes = br"format!(\"{}\", path.display())";
        let byte = format!("{}", b'(');
        my_format!("{}", path.display());
"###;
    assert_eq!(display_in_format(source), [] as [usize; 0]);
}

#[test]
fn the_tests_of_this_crate_embed_paths_through_sh_path() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut files = Vec::new();
    rust_files(&dir, &mut files);
    files.sort();
    let found: Vec<String> = files
        .iter()
        .flat_map(|path| {
            let source = std::fs::read_to_string(path).unwrap();
            let shown = path.strip_prefix(&dir).unwrap().display().to_string();
            display_in_format(&source)
                .into_iter()
                .map(move |line| format!("tests/{shown}:{line}"))
        })
        .collect();
    assert!(
        found.is_empty(),
        "format! の引数に Path::display() を直に使っています（Windows では \\ で区切り、sh のコマンドの中で消え、\
         git の書く / 区切りのパスと合いません）。common::sh_path を通してください。出力が渡したパスをそのまま返す\
         なら、渡した文字列（to_str()）と比べます。失敗の文なら panic! や assert! の引数に直に書きます:\n{}",
        found.join("\n")
    );
}
