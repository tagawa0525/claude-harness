//! 旧と新のコミットの番号の対応表を読み、文書の中の番号を置き換える（ADR-0012）。
//!
//! 文書の中の、7 桁以上 64 桁以下で単語の境界に挟まれた小文字の 16 進の語のうち、対応表の旧の番号の先頭に
//! 一致するものを、同じ桁数の新の番号の先頭に置き換える。対応表に無い語は変えない。単語の境界は ASCII の
//! 英数字と `_` で決める（ASCII でないバイトは単語の外）。

use std::collections::{BTreeMap, HashMap};

/// 照合の索引に使う先頭の桁数（語の最短の長さ）
const PREFIX: usize = 7;
const MAX_TOKEN: usize = 64;

/// 対応表の旧が、書き換えでどうなったか。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// 新の番号に変わった
    Changed(String),
    /// 書き換えで消えた
    Pruned,
    /// 変わらなかった
    Unchanged,
}

/// 対応表。
#[derive(Debug, Default)]
pub struct Map {
    /// 旧 → 書き換えでどうなったか
    targets: BTreeMap<String, Target>,
    /// 旧の先頭 7 桁 → 旧（対応表のすべての旧。変わらなかったものも入れるのは、短い語がそれと変わったものの
    /// 両方に当たるときに、曖昧として止めるため）
    index: HashMap<String, Vec<String>>,
}

impl Map {
    /// 対応表を読む。1 行に「旧 新」の完全な番号（3 列目以降は無視する）。最初の行が `old new` なら見出しとして
    /// 読み飛ばす（git filter-repo の commit-map）。新が 0 だけの行は消えたコミット、旧と新が同じ行は変わらなかった
    /// コミット。行が読めないとき、旧と新の桁数が違うとき、同じ旧に別々の新が対応するとき、新が別の行の旧として
    /// 変わる（連鎖する）ときは、すべての理由を返す。
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
            // 置き換えは文書の語と同じ桁数の新の先頭を使うので、新が旧より短いと足りなくなる。0 だけの印は
            // 消えたコミットを表すだけで置き換えに使わないので、桁数を問わない
            if old.len() != new.len() && !is_zero(new) {
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
        // 新が別の行の（変わる）旧でもある連鎖は、1 回の置き換えでは最後の新にならず、書き込みの途中で失敗した
        // 後に呼び直すと、書き換え済みの語をもう一度書き換える
        for (old, new) in &targets {
            if let Some(next) = targets.get(new)
                && next != new
                && old != new
            {
                errors.push(format!(
                    "対応が連鎖している: {old} → {new} → {next}（合成した対応表を渡す）"
                ));
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }

        let mut map = Map::default();
        for (old, new) in targets {
            let target = if is_zero(new) {
                Target::Pruned
            } else if old != new {
                Target::Changed(new.to_string())
            } else {
                Target::Unchanged
            };
            map.insert(old, target);
        }
        Ok(map)
    }

    /// 対応表に無いコミットを、変わらなかったものとして照合に足す（すでにあるものはそのまま）。
    pub fn add_unchanged<'a>(&mut self, commits: impl IntoIterator<Item = &'a str>) {
        for commit in commits {
            if !self.targets.contains_key(commit) {
                self.insert(commit, Target::Unchanged);
            }
        }
    }

    /// 旧と、書き換えでどうなったかの組
    pub fn targets(&self) -> impl Iterator<Item = (&str, &Target)> {
        self.targets
            .iter()
            .map(|(old, target)| (old.as_str(), target))
    }

    fn insert(&mut self, old: &str, target: Target) {
        self.index
            .entry(old[..PREFIX].to_string())
            .or_default()
            .push(old.to_string());
        self.targets.insert(old.to_string(), target);
    }

    /// `data` の中の旧の番号を置き換えた内容と、置き換えた語の数を返す。曖昧な語と、消えたコミットを指す語は
    /// 置き換えずに、`label` を添えた理由を `errors` に積む。
    pub fn replace(&self, label: &str, data: &[u8], errors: &mut Vec<String>) -> (Vec<u8>, usize) {
        self.replace_by(
            label,
            data,
            errors,
            |token, old, target, errors| match target {
                Target::Changed(new) => Some(new.clone()),
                Target::Pruned => {
                    errors.push(format!(
                        "{label}: {token} は書き換えで消えたコミット {old} を指す"
                    ));
                    None
                }
                // 変わらなかったコミットを指す語は、そのまま残す
                Target::Unchanged => None,
            },
        )
    }

    /// `data` の中の、対応表の旧に 1 つだけ当たる語を、`resolve` が返す新の番号の先頭（語と同じ桁数）に置き換える。
    /// `resolve` は文書の語と、それが当たった旧と、旧が書き換えでどうなったかを受け、置き換えない語には None を返す（止める理由は `errors`
    /// に積む）。曖昧な語は置き換えずに、`label` を添えた理由を `errors` に積む。置き換えた内容と、変わった語の数を
    /// 返す。
    pub fn replace_by(
        &self,
        label: &str,
        data: &[u8],
        errors: &mut Vec<String>,
        mut resolve: impl FnMut(&str, &str, &Target, &mut Vec<String>) -> Option<String>,
    ) -> (Vec<u8>, usize) {
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
            // 当たる旧があるのは小文字の 16 進だけの語なので、UTF-8 として読める
            let new = self.lookup(label, word, errors).and_then(|old| {
                let token = std::str::from_utf8(word).unwrap_or_default();
                resolve(token, old, &self.targets[old], errors)
            });
            match new {
                Some(new) if new.as_bytes()[..word.len()] != *word => {
                    out.extend_from_slice(&new.as_bytes()[..word.len()]);
                    count += 1;
                }
                _ => out.extend_from_slice(word),
            }
        }
        (out, count)
    }

    /// 単語 `word` が対応表の旧に 1 つだけ当たるなら、その旧を返す。
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
            [old] => Some(old.as_str()),
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

/// 消えたコミットの印（0 だけの番号）か
fn is_zero(s: &str) -> bool {
    s.bytes().all(|b| b == b'0')
}

/// SHA-1（40 桁）か SHA-256（64 桁）の、小文字の 16 進の番号か
pub fn is_full_hash(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && s.bytes().all(is_lower_hex)
}

fn is_lower_hex(b: u8) -> bool {
    b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
}

/// 単語の境界を決めるバイト（正規表現の `\b` を ASCII で読んだもの）
fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}
