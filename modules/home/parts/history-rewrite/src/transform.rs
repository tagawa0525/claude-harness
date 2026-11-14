//! 第 1 段の変換（ADR-0045）。ブロブと本文に辞書の規則を当て、直した Markdown の表を揃え直す

use crate::Stop;
use crate::glob::Glob;
use ja_terms::Dictionary;
use std::path::PathBuf;

/// git の buffer_is_binary（xdiff-interface.c の FIRST_FEW_BYTES）と同じ、テキストかどうかを見る先頭のバイト数。
/// commit-refs rewrite-history（ADR-0013）も同じ判定を使う
pub const FIRST_FEW_BYTES: usize = 8000;

/// 語の指定（`--dictionary`、`--extra-rules`、`--exclude`）
#[derive(Debug, Default, Clone)]
pub struct Words {
    pub dictionaries: Vec<PathBuf>,
    pub extra_rules: Vec<PathBuf>,
    pub excludes: Vec<String>,
}

pub struct Transformer {
    dictionary: Dictionary,
    excludes: Vec<Glob>,
    /// UTF-8 として読めないので直さなかったテキストのブロブの数
    pub not_utf8: usize,
}

impl Transformer {
    /// 辞書を渡した順に読む。`--dictionary` が無ければ ja-terms の辞書（ビルドに埋めたもの）
    pub fn new(words: &Words) -> Result<Transformer, Stop> {
        let load = |path: &PathBuf| Dictionary::load(path).map_err(|e| Stop(e.to_string()));
        let mut given = words.dictionaries.iter().chain(&words.extra_rules);
        let mut dictionary = if words.dictionaries.is_empty() {
            Dictionary::parse(ja_terms::DICTIONARY, "ja-terms の prh.yml")
                .map_err(|e| Stop(e.to_string()))?
        } else {
            load(given.next().expect("dictionaries が空でない"))?
        };
        for path in given {
            dictionary.append(load(path)?);
        }
        Ok(Transformer {
            dictionary,
            excludes: words.excludes.iter().map(|g| Glob::new(g)).collect(),
            not_utf8: 0,
        })
    }

    pub fn excluded(&self, path: &str) -> bool {
        self.excludes.iter().any(|g| g.matches(path))
    }

    /// ブロブを読まない理由（excluded、binary、not UTF-8）。読むなら None
    pub fn skipped(&self, path: &[u8], data: &[u8]) -> Option<&'static str> {
        if self.excluded(&String::from_utf8_lossy(path)) {
            return Some("excluded");
        }
        if data[..data.len().min(FIRST_FEW_BYTES)].contains(&0) {
            return Some("binary");
        }
        if std::str::from_utf8(data).is_err() {
            return Some("not UTF-8");
        }
        None
    }

    /// パス path のブロブの新しい中身。変えないなら None
    pub fn blob(&mut self, path: &[u8], data: &[u8]) -> Result<Option<Vec<u8>>, Stop> {
        match self.skipped(path, data) {
            Some("not UTF-8") => {
                self.not_utf8 += 1;
                return Ok(None);
            }
            Some(_) => return Ok(None),
            None => {}
        }
        let name = String::from_utf8_lossy(path);
        let text = std::str::from_utf8(data).expect("skipped が UTF-8 を確かめた");
        let new = self.text(&name, text, name.to_lowercase().ends_with(".md"))?;
        Ok((new != text).then(|| new.into_bytes()))
    }

    /// コミットかタグ name の本文。本文は Markdown として表も揃える
    pub fn message(&self, data: &[u8], name: &str) -> Result<Vec<u8>, Stop> {
        let text = std::str::from_utf8(data)
            .map_err(|e| Stop(format!("{name}: UTF-8 として読めない本文がある: {e}")))?;
        Ok(self.text(name, text, true)?.into_bytes())
    }

    /// 本文 text の新しい中身（Markdown なら直した行を含む表を揃え直す）
    pub fn text(&self, name: &str, text: &str, markdown: bool) -> Result<String, Stop> {
        let new = self
            .dictionary
            .apply(text)
            .map_err(|e| Stop(format!("{name}: {e}")))?;
        if markdown && new != text {
            return realign_tables(name, text, &new);
        }
        Ok(new)
    }
}

/// new のうち、old から直した行を含む表だけを揃え直す（ほかの表の崩れは、この書き換えの範囲の外）。
/// fix-markdown-lint の表の段と同じく、コードフェンスの中を飛ばし、区切りの行が続く行から表とみなす
fn realign_tables(name: &str, old: &str, new: &str) -> Result<String, Stop> {
    let old_lines: Vec<&str> = old.split('\n').collect();
    let lines: Vec<&str> = new.split('\n').collect();
    if old_lines.len() != lines.len() {
        // 行の対応が取れないと、どの表を直したか決まらない
        return Err(Stop(format!(
            "{name}: 置き換えで行の数が変わる（表の揃え直しは、行の数を変えない置き換えだけを扱う）"
        )));
    }
    let changed = |k: usize| old_lines[k] != lines[k];
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        if fix_markdown_lint::fence_open(lines[i]).is_some() {
            let end = fix_markdown_lint::fence_end(&lines, i);
            let stop = (end + 1).min(lines.len());
            out.extend(lines[i..stop].iter().map(|l| l.to_string()));
            i = stop;
            continue;
        }
        if !(fix_markdown_lint::is_table_row(lines[i])
            && i + 1 < lines.len()
            && fix_markdown_lint::is_separator_row(lines[i + 1]))
        {
            out.push(lines[i].to_string());
            i += 1;
            continue;
        }
        let mut j = i;
        while j < lines.len() && fix_markdown_lint::is_table_row(lines[j]) {
            j += 1;
        }
        let rows = &lines[i..j];
        if !(i..j).any(changed) {
            out.extend(rows.iter().map(|l| l.to_string()));
            i = j;
            continue;
        }
        // 表の揃え方は \| でも列を切るので、hook と同じ形に揃え直せない表は止める。字下げした表も止める（揃え方は
        // 字下げを保つが、履歴の書き換えで字下げした表を揃え直した結果を確かめるテストが無い）
        for row in rows {
            if row.contains("\\|") {
                return Err(Stop(format!(
                    "{name}: 直す表の行に \\| がある（揃え直せない）: {}",
                    py_strip(row)
                )));
            }
            if row.chars().next().is_some_and(is_py_space) {
                return Err(Stop(format!(
                    "{name}: 直す表が字下げしてある（揃え直せない）: {}",
                    py_strip(row)
                )));
            }
        }
        // 表の揃え方は行の両端の空白とともに CRLF の CR も外すので、元の行の CR を戻す
        let formatted = fix_markdown_lint::format_table(rows);
        for (row, original) in formatted.into_iter().zip(rows) {
            let cr = if original.ends_with('\r') { "\r" } else { "" };
            out.push(format!("{row}{cr}"));
        }
        i = j;
    }
    Ok(out.join("\n"))
}

/// 表の揃え方（fix-markdown-lint）と同じ空白（Unicode の White_Space と \x1c〜\x1f）
fn is_py_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

fn py_strip(s: &str) -> &str {
    s.trim_matches(is_py_space)
}
