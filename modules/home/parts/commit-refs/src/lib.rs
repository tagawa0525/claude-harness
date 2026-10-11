//! 旧と新のコミットの番号の対応表を読み、文書の中の番号を置き換える（ADR-0012）。
//!
//! 文書の中の、7 桁以上 64 桁以下で単語の境界に挟まれた小文字の 16 進の語のうち、対応表の旧の番号の先頭に
//! 一致するものを、同じ桁数の新の番号の先頭に置き換える。対応表に無い語は変えない。単語の境界は ASCII の
//! 英数字と `_` で決める（ASCII でないバイトは単語の外）。

use std::collections::{BTreeMap, BTreeSet, HashMap};

/// 照合の索引に使う先頭の桁数（語の最短の長さ）
const PREFIX: usize = 7;
const MAX_TOKEN: usize = 64;

/// 対応表。
#[derive(Debug, Default)]
pub struct Map {
    /// 変わったコミットの旧 → 新
    mapping: BTreeMap<String, String>,
    /// 書き換えで消えたコミットの旧
    pruned: BTreeSet<String>,
    /// 旧の先頭 7 桁 → 旧（変わったものと消えたもの。曖昧さの判定と消えたコミットの検出に使う）
    index: HashMap<String, Vec<String>>,
}

impl Map {
    /// 対応表を読む。1 行に「旧 新」の完全な番号（3 列目以降は無視する）。最初の行が `old new` なら見出しとして
    /// 読み飛ばす（git filter-repo の commit-map）。新が 0 だけの行は消えたコミット、旧と新が同じ行は変わらなかった
    /// コミット。行が読めないとき、旧と新の桁数が違うとき、同じ旧に別々の新が対応するときは、すべての理由を返す。
    pub fn parse(text: &str) -> Result<Map, Vec<String>> {
        let mut errors = Vec::new();
        let mut targets: BTreeMap<&str, &str> = BTreeMap::new();
        let mut seen_entry = false;
        for (lineno, line) in text.lines().enumerate() {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.is_empty() {
                continue;
            }
            if !seen_entry && fields == ["old", "new"] {
                continue;
            }
            seen_entry = true;
            if fields.len() < 2 || !is_full_hash(fields[0]) || !is_full_hash(fields[1]) {
                errors.push(format!(
                    "対応表の {} 行目が「旧 新」の完全な番号でない: {}",
                    lineno + 1,
                    line.trim_end()
                ));
                continue;
            }
            let (old, new) = (fields[0], fields[1]);
            // 置き換えは文書の語と同じ桁数の新の先頭を使うので、新が旧より短いと足りなくなる
            if old.len() != new.len() {
                errors.push(format!(
                    "対応表の {} 行目の旧と新の桁数が違う: {}",
                    lineno + 1,
                    line.trim_end()
                ));
                continue;
            }
            // 変わらない行と消えた行も含めて、旧ごとの新を先に揃える
            let first = *targets.entry(old).or_insert(new);
            if first != new {
                errors.push(format!("旧 {old} に別々の新が対応する: {first} と {new}"));
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }

        let mut map = Map::default();
        for (old, new) in targets {
            if new.bytes().all(|b| b == b'0') {
                map.pruned.insert(old.to_string());
            } else if old != new {
                map.mapping.insert(old.to_string(), new.to_string());
            }
        }
        for old in map.mapping.keys().chain(&map.pruned) {
            map.index
                .entry(old[..PREFIX].to_string())
                .or_default()
                .push(old.clone());
        }
        Ok(map)
    }

    /// `data` の中の旧の番号を置き換えた内容と、置き換えた語の数を返す。曖昧な語と、消えたコミットを指す語は
    /// 置き換えずに、`label` を添えた理由を `errors` に積む。
    pub fn replace(&self, label: &str, data: &[u8], errors: &mut Vec<String>) -> (Vec<u8>, usize) {
        let mut out = Vec::with_capacity(data.len());
        let mut count = 0;
        let mut i = 0;
        while i < data.len() {
            if !is_word(data[i]) {
                out.push(data[i]);
                i += 1;
                continue;
            }
            let start = i;
            while i < data.len() && is_word(data[i]) {
                i += 1;
            }
            let word = &data[start..i];
            match self.lookup(label, word, errors) {
                Some(new) => {
                    out.extend_from_slice(&new.as_bytes()[..word.len()]);
                    count += 1;
                }
                None => out.extend_from_slice(word),
            }
        }
        (out, count)
    }

    /// 単語 `word` が置き換える語なら、その新の番号を返す。
    fn lookup(&self, label: &str, word: &[u8], errors: &mut Vec<String>) -> Option<&str> {
        if !(PREFIX..=MAX_TOKEN).contains(&word.len()) || !word.iter().all(|b| is_lower_hex(*b)) {
            return None;
        }
        // 小文字の 16 進だけなので UTF-8 として読める
        let token = std::str::from_utf8(word).ok()?;
        let olds: Vec<&String> = self
            .index
            .get(&token[..PREFIX])?
            .iter()
            .filter(|old| old.starts_with(token))
            .collect();
        match olds.as_slice() {
            [] => None,
            [old] if self.pruned.contains(*old) => {
                errors.push(format!(
                    "{label}: {token} は書き換えで消えたコミット {old} を指す"
                ));
                None
            }
            [old] => Some(&self.mapping[*old]),
            _ => {
                let names: Vec<&str> = olds.iter().map(|o| o.as_str()).collect();
                errors.push(format!(
                    "{label}: {token} が複数の旧に当たる（曖昧）: {}",
                    names.join(" ")
                ));
                None
            }
        }
    }
}

/// SHA-1（40 桁）か SHA-256（64 桁）の、小文字の 16 進の番号か
fn is_full_hash(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && s.bytes().all(is_lower_hex)
}

fn is_lower_hex(b: u8) -> bool {
    b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
}

/// 単語の境界を決めるバイト（正規表現の `\b` を ASCII で読んだもの）
fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}
