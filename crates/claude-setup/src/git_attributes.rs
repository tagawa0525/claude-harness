//! `claude-setup git-attributes --from <ファイル>`: git のグローバルの attributes を置く（ADR-0105。Linux は
//! git-attributes.nix が置く）。
//!
//! git が core.attributesFile の無いときに読む `$XDG_CONFIG_HOME/git/attributes`（無いか空なら
//! `~/.config/git/attributes`）に、ファイルの中身を書く。core.attributesFile が別の場所を指すとき（git はそこだけを読み、
//! 置いても効かない）と、置く場所にハーネスのものでないファイルがあるときは、黙って奪わずに止める

use std::path::{Path, PathBuf};

use crate::git_hooks::{config_value, git_home, same_path};

pub fn run(home: &Path, from: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(from)
        .map_err(|e| format!("❌ {} を読めません（{e}）", from.display()))?;
    let path = target(home);
    // 置く場所の親を先に作る: core.attributesFile の値と比べるとき、まだ無いファイルは親を正規化して比べる
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("❌ {} を作れません（{e}）", dir.display()))?;
    }
    // git が使うのは global の値で、無ければ system の値
    let set = match config_value("--global", "core.attributesFile")? {
        Some(value) => Some(("--global", value)),
        None => config_value("--system", "core.attributesFile")?.map(|value| ("--system", value)),
    };
    if let Some((scope, value)) = set
        && !same_path(home, &value, &path)
    {
        return Err(format!(
            "❌ {scope} の core.attributesFile は {value} を指しています。git はそこだけを読み、{} に置いても効かないので、ここで止めます\n   修正方法: 改行の規則（{}）をそのファイルに足すか、そのファイルの規則を移してから core.attributesFile を外し、もう一度実行してください",
            path.display(),
            from.display()
        ));
    }
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Ok(meta) if !meta.file_type().is_symlink() => match std::fs::read_to_string(&path) {
            Ok(old) if old == text => return Ok(()),
            Ok(old) if same_first_line(&old, &text) => {}
            // 読めないファイルと、利用者の書いたファイルは持ち主のもの
            _ => return Err(foreign(&path, from)),
        },
        // symlink（Linux の home-manager が store に置くもの）は持ち主のもの
        _ => return Err(foreign(&path, from)),
    }
    crate::sync::replace_with(&path, |tmp| std::fs::write(tmp, &text))
        .map_err(|e| format!("❌ {} を書けません（{e}）", path.display()))
}

/// git が既定で読むグローバルの attributes の場所（gitattributes(5)）
fn target(home: &Path) -> PathBuf {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| git_home(home).join(".config"));
    config.join("git").join("attributes")
}

/// ハーネスが置いたファイルか。配る中身の 1 行目（変えない約束。ADR-0105）で見分ける
fn same_first_line(old: &str, text: &str) -> bool {
    matches!((old.lines().next(), text.lines().next()), (Some(a), Some(b)) if a == b)
}

fn foreign(path: &Path, from: &Path) -> String {
    format!(
        "❌ {} に、ハーネスのものでない attributes があります。上書きするとその規則が効かなくなるので、ここで止めます\n   修正方法: その規則をリポジトリの .gitattributes か $GIT_DIR/info/attributes に移してファイルを外すか、改行の規則（{}）を自分でそのファイルに足して、この手順を飛ばしてください",
        path.display(),
        from.display()
    )
}
