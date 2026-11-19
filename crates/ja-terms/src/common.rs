//! 共通の辞書の置き場（ADR-0097）。`claude-setup sync` がソースの `crates/ja-terms/prh.yml` をホームの下の
//! [`COMMON_DICTIONARY`] に置き、ja-terms と history-rewrite はそこを読む。ビルドに埋めないので、辞書の規則を足すと、
//! ビルドし直さずに sync だけで効く。

use std::path::PathBuf;

use crate::DictionaryError;

/// 共通の辞書の、ホームからのパス（`claude-setup sync` の HOME_FILES の置き先と同じ）
pub const COMMON_DICTIONARY: &str = ".config/ja-terms/prh.yml";

/// 共通の辞書のパスを差し替える環境変数。ホームに辞書の無いテストとビルドの中で、crate の辞書を渡すためのもの
pub const COMMON_DICTIONARY_ENV: &str = "JA_TERMS_COMMON_DICTIONARY";

/// 共通の辞書のパス。環境変数が空でなければそのパス、無ければホームの下
///
/// ホームは `claude-setup sync` と同じ `std::env::home_dir`（Unix は HOME、Windows は USERPROFILE）で決める。
/// `XDG_CONFIG_HOME` は見ない（sync は見ないので、置く場所と読む場所がずれる）
pub fn path() -> Result<PathBuf, DictionaryError> {
    if let Some(path) = std::env::var_os(COMMON_DICTIONARY_ENV).filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::home_dir()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| {
            DictionaryError(format!(
                "共通の辞書（~/{COMMON_DICTIONARY}）を探すホームのディレクトリが分からない（HOME、Windows では USERPROFILE）"
            ))
        })?;
    Ok(home.join(COMMON_DICTIONARY))
}

/// 共通の辞書のパスと中身。無ければ、配り方を添えて止める（埋めた辞書や空の辞書にすり替えない）
pub fn read() -> Result<(PathBuf, String), DictionaryError> {
    let path = path()?;
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok((path, text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(DictionaryError(format!(
            "共通の辞書が無い: {}。claude-sync（Windows では claude-setup sync）で配る（テストとビルドの中では {COMMON_DICTIONARY_ENV} に辞書のパスを渡す）",
            path.display()
        ))),
        Err(e) => Err(DictionaryError(format!(
            "{}: 読めない: {e}",
            path.display()
        ))),
    }
}
