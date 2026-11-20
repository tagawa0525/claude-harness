//! ステージ 1 の変換（ADR-0045）。ブロブと本文に辞書の規則を当て、直した Markdown の表を揃え直す

use crate::Stop;
use crate::glob::Glob;
use ja_terms::Dictionary;
use std::path::{Path, PathBuf};

/// git の buffer_is_binary（xdiff-interface.c の FIRST_FEW_BYTES）と同じ、テキストかどうかを見る先頭のバイト数。
/// commit-refs rewrite-history（ADR-0013）も同じ判定を使う
pub const FIRST_FEW_BYTES: usize = 8000;

/// 語の指定（`--dictionary`、`--repo-dictionary`、`--extra-rules`、`--exclude`）
#[derive(Debug, Default, Clone)]
pub struct Words {
    pub dictionaries: Vec<PathBuf>,
    /// 共通の辞書（`--dictionary` があればそれ）に重ねるリポジトリの辞書（ADR-0096）。[`Words::with_repo_dictionary`] が
    /// 作業ツリーから見つけるか、`--repo-dictionary` で渡す
    pub repo_dictionaries: Vec<PathBuf>,
    pub extra_rules: Vec<PathBuf>,
    pub excludes: Vec<String>,
}

/// 記録の行の種類と、Words の列
const RECORD_KEYS: [&str; 4] = ["dictionary", "repo-dictionary", "extra-rules", "exclude"];

impl Words {
    pub fn is_empty(&self) -> bool {
        self.dictionaries.is_empty()
            && self.repo_dictionaries.is_empty()
            && self.extra_rules.is_empty()
            && self.excludes.is_empty()
    }

    /// 辞書を渡されていなければ（`--dictionary` も `--repo-dictionary` も無い）、dir を含む作業ツリーの直下の
    /// リポジトリの辞書（ja-terms の検査と同じ `.ja-terms/prh.yml`）を足す。作業ツリーの外か、辞書が無ければそのまま
    pub fn with_repo_dictionary(mut self, dir: &Path) -> Result<Words, Stop> {
        if !self.dictionaries.is_empty() || !self.repo_dictionaries.is_empty() {
            return Ok(self);
        }
        if let Some(path) = ja_terms::repo::dictionary_path(dir).map_err(|e| Stop(e.to_string()))? {
            self.repo_dictionaries.push(path);
        }
        Ok(self)
    }

    /// 辞書と追加の規則を実体のパスにした指定（cwd の違う run と sync-worktrees で、同じファイルを同じと比べるため）
    pub fn resolved(&self) -> Result<Words, Stop> {
        let real = |paths: &[PathBuf]| -> Result<Vec<PathBuf>, Stop> {
            paths
                .iter()
                .map(|p| crate::real_path(p).map_err(|e| Stop(format!("{}: {e}", p.display()))))
                .collect()
        };
        Ok(Words {
            dictionaries: real(&self.dictionaries)?,
            repo_dictionaries: real(&self.repo_dictionaries)?,
            extra_rules: real(&self.extra_rules)?,
            excludes: self.excludes.clone(),
        })
    }

    /// 同じ変換になる指定か。辞書と追加の規則は当てる順が意味を持つので順に比べ、除くパスは集まりとして比べる
    pub fn same(&self, other: &Words) -> bool {
        let sorted = |w: &Words| {
            let mut e = w.excludes.clone();
            e.sort();
            e
        };
        self.dictionaries == other.dictionaries
            && self.repo_dictionaries == other.repo_dictionaries
            && self.extra_rules == other.extra_rules
            && sorted(self) == sorted(other)
    }

    fn columns(&self) -> [Vec<String>; 4] {
        let shown = |paths: &[PathBuf]| paths.iter().map(|p| p.display().to_string()).collect();
        [
            shown(&self.dictionaries),
            shown(&self.repo_dictionaries),
            shown(&self.extra_rules),
            self.excludes.clone(),
        ]
    }

    /// コマンドの引数の形（`--extra-rules FILE --exclude GLOB`）。空なら「なし」
    pub fn args(&self) -> String {
        let mut out = Vec::new();
        for (key, values) in RECORD_KEYS.iter().zip(self.columns()) {
            for v in values {
                out.push(format!(
                    "--{key} {}",
                    crate::sh_quote(std::path::Path::new(&v))
                ));
            }
        }
        if out.is_empty() {
            "なし".to_string()
        } else {
            out.join(" ")
        }
    }

    /// 記録の中身（1 行に「種類 TAB 値」）。値に改行かタブがあれば、行に収まらないので止める
    pub fn record(&self) -> Result<String, Stop> {
        let mut out = String::new();
        for (key, values) in RECORD_KEYS.iter().zip(self.columns()) {
            for v in values {
                if v.contains(['\n', '\r', '\t']) {
                    return Err(Stop(format!(
                        "語の指定に改行かタブを含む値がある（作業ディレクトリに記録できない）: {v:?}"
                    )));
                }
                out.push_str(&format!("{key}\t{v}\n"));
            }
        }
        Ok(out)
    }

    /// record の中身を読む
    pub fn parse_record(text: &str, name: &str) -> Result<Words, Stop> {
        let mut words = Words::default();
        for (k, line) in text.lines().enumerate() {
            match line.split_once('\t') {
                Some(("dictionary", v)) => words.dictionaries.push(PathBuf::from(v)),
                Some(("repo-dictionary", v)) => words.repo_dictionaries.push(PathBuf::from(v)),
                Some(("extra-rules", v)) => words.extra_rules.push(PathBuf::from(v)),
                Some(("exclude", v)) => words.excludes.push(v.to_string()),
                _ => {
                    return Err(Stop(format!(
                        "{name}:{}: 語の指定の記録の行でない: {line:?}",
                        k + 1
                    )));
                }
            }
        }
        Ok(words)
    }
}

pub struct Transformer {
    dictionary: Dictionary,
    excludes: Vec<Glob>,
    /// UTF-8 として読めないので直さなかったテキストのブロブの数
    pub not_utf8: usize,
}

impl Transformer {
    /// 辞書を渡した順に読む。`--dictionary` が無ければ ja-terms の共通の辞書（`claude-setup sync` が配ったもの。ADR-0097）。リポジトリの辞書が
    /// あれば、その後ろに共通の辞書を prh の Engine.merge でつなぎ（ja-terms の検査と同じ。ADR-0096）、追加の規則は
    /// 最後に並べる
    pub fn new(words: &Words) -> Result<Transformer, Stop> {
        Self::with_common(words, None)
    }

    /// [`Transformer::new`] の、`--dictionary` が無いときの共通の辞書を渡せる形（None なら配った場所から読む）。run は
    /// 読んだ共通の辞書の中身を作業ディレクトリにコピーするので、同じ中身から変換を作る（読み直すと、間に配り直された
    /// 辞書になりうる。ADR-0097）
    pub fn with_common(words: &Words, common: Option<Dictionary>) -> Result<Transformer, Stop> {
        let load = |path: &PathBuf| Dictionary::load(path).map_err(|e| Stop(e.to_string()));
        let mut given = words.dictionaries.iter();
        let mut base = match (given.next(), common) {
            (Some(first), _) => load(first)?,
            (None, Some(common)) => common,
            (None, None) => Dictionary::common().map_err(|e| Stop(e.to_string()))?,
        };
        for path in given {
            base.append(load(path)?);
        }
        let mut dictionary = match words.repo_dictionaries.split_first() {
            None => base,
            Some((first, rest)) => {
                let mut repo = load(first)?;
                for path in rest {
                    repo.merge(load(path)?);
                }
                repo.merge(base);
                repo
            }
        };
        for path in &words.extra_rules {
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
/// 表の範囲は fix-markdown-lint と同じ読み方で決める（[`fix_markdown_lint::tables`]）
fn realign_tables(name: &str, old: &str, new: &str) -> Result<String, Stop> {
    let old_lines: Vec<&str> = old.split('\n').collect();
    let lines: Vec<&str> = new.split('\n').collect();
    if old_lines.len() != lines.len() {
        // 行の対応が取れないと、どの表を直したか決まらない
        return Err(Stop(format!(
            "{name}: 置き換えで行の数が変わる（表の揃え直しは、行の数を変えない置き換えだけを扱う）"
        )));
    }
    let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    for range in fix_markdown_lint::tables(new) {
        if !range.clone().any(|k| old_lines[k] != lines[k]) {
            continue;
        }
        // 表の揃え方は行の両端の空白とともに CRLF の CR も外すので、元の行の CR を戻す
        let rows = &lines[range.clone()];
        for (k, row) in range.zip(fix_markdown_lint::format_table(rows)) {
            let cr = if lines[k].ends_with('\r') { "\r" } else { "" };
            out[k] = format!("{row}{cr}");
        }
    }
    Ok(out.join("\n"))
}
