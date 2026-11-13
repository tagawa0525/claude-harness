//! `ja-terms staged`: ステージしたファイルの崩れた日本語の語を検査する（ADR-0014、ADR-0085。前の staged.sh と同じ
//! 判定）。グローバルの pre-commit が呼ぶ。
//!
//! - 見るのはステージした中身（作業ツリーではない）の、Markdown とテキストとコードのファイル（拡張子は
//!   [`ja_terms::report::syntax_of`]。大文字と小文字を問わない）。prh の辞書（prh.yml）は誤った語を並べるので外す
//! - symlink の行き先とサブモジュール（gitlink）は文章ではないので外す
//! - ステージした `.textlintignore` に挙げたファイルを外す。書き方は gitignore と同じなので、git 自身に当てさせる
//!
//! 出力は `ja-terms check` と同じ形で、パスはリポジトリの直下からのもの（どのディレクトリで呼んでも同じ）。終了コード: 0 = 崩れた語が無い、
//! 1 = 崩れた語がある、2 = 検査できなかった

use std::collections::HashSet;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use ja_terms::Dictionary;
use ja_terms::report::{self, TEXT_EXTENSIONS};

use crate::{Failure, print};

/// git を実行し、標準出力を返す。top は -C で渡すディレクトリ（リポジトリの直下）
fn git(top: Option<&Path>, args: &[&str]) -> Result<Vec<u8>, Failure> {
    let mut command = Command::new("git");
    if let Some(top) = top {
        command.arg("-C").arg(top);
    }
    let out = command
        .args(args)
        .output()
        .map_err(|e| Failure(format!("git を起動できない: {e}")))?;
    if !out.status.success() {
        return Err(Failure(format!(
            "git {} が失敗した（{}）: {}",
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(out.stdout)
}

/// ステージしたファイルの (パス, ブロブ)。symlink とサブモジュールを除く
fn staged_files(top: &Path) -> Result<Vec<(String, String)>, Failure> {
    let mut pathspecs: Vec<String> = ["md"]
        .iter()
        .chain(TEXT_EXTENSIONS.iter())
        .map(|ext| format!(":(icase,glob)**/*.{ext}"))
        .collect();
    pathspecs.push(":(exclude,glob)**/prh.yml".to_string());
    // --raw -z は「:旧モード 新モード 旧ブロブ 新ブロブ 状態」と「パス」を NUL で区切って並べる。名前の変更は
    // パスを 2 つ並べるので、--no-renames で追加と削除に分けて 1 つにする。T は symlink などから普通の
    // ファイルへの種類の変更
    let mut args = vec![
        "diff",
        "--cached",
        "--raw",
        "-z",
        "--no-abbrev",
        "--no-renames",
        "--diff-filter=ACMRT",
        "--",
    ];
    args.extend(pathspecs.iter().map(String::as_str));
    let out = git(Some(top), &args)?;
    let mut fields = out.split(|&b| b == 0).filter(|f| !f.is_empty());
    let mut files = Vec::new();
    while let (Some(meta), Some(path)) = (fields.next(), fields.next()) {
        let meta = String::from_utf8_lossy(meta);
        let parts: Vec<&str> = meta.split_whitespace().collect();
        let [_, mode, _, blob, _] = parts.as_slice() else {
            return Err(Failure(format!("git diff --raw の行を読めない: {meta}")));
        };
        if matches!(*mode, "120000" | "160000") {
            continue;
        }
        files.push((String::from_utf8_lossy(path).into_owned(), blob.to_string()));
    }
    Ok(files)
}

/// ステージした .textlintignore に当たる、インデックスのファイルのパス
fn ignored(top: &Path) -> Result<HashSet<String>, Failure> {
    if git(Some(top), &["cat-file", "-e", ":.textlintignore"]).is_err() {
        return Ok(HashSet::new());
    }
    let patterns = git(Some(top), &["show", ":.textlintignore"])?;
    // --exclude-from はファイルを読むので、git dir の中（ほかの利用者が先に置けない）に、無い名前で作って書く
    let (path, mut file) = private_file(top)?;
    let written = file.write_all(&patterns);
    drop(file);
    if let Err(e) = written {
        let _ = std::fs::remove_file(&path);
        return Err(Failure(format!("{} に書けない: {e}", path.display())));
    }
    let exclude = format!("--exclude-from={}", path.display());
    let listed = git(
        Some(top),
        &["ls-files", "-z", "--cached", "--ignored", &exclude],
    );
    let _ = std::fs::remove_file(&path);
    Ok(listed?
        .split(|&b| b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect())
}

/// git dir の中に、まだ無い名前の空のファイルを作って開く（既にある名前の symlink を辿って書かない）
fn private_file(top: &Path) -> Result<(PathBuf, File), Failure> {
    let dir = git(Some(top), &["rev-parse", "--absolute-git-dir"])?;
    let dir = PathBuf::from(String::from_utf8_lossy(&dir).trim_end_matches('\n'));
    for n in 0.. {
        let path = dir.join(format!(
            "ja-terms-textlintignore-{}-{n}",
            std::process::id()
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(Failure(format!("{} を作れない: {e}", path.display()))),
        }
    }
    unreachable!("名前の数は尽きない")
}

pub(crate) fn run(dictionary: &Dictionary) -> Result<u8, Failure> {
    // git diff --cached --raw は直下からのパスを出すが、pathspec と git ls-files は今のディレクトリから読むので、
    // どの git も直下で実行する
    let top = git(None, &["rev-parse", "--show-toplevel"])?;
    let top = PathBuf::from(String::from_utf8_lossy(&top).trim_end_matches('\n'));
    let files = staged_files(&top)?;
    if files.is_empty() {
        return Ok(0);
    }
    let ignored = ignored(&top)?;
    let files: Vec<_> = files
        .into_iter()
        .filter(|(path, _)| !ignored.contains(path))
        .collect();
    if files.is_empty() {
        return Ok(0);
    }
    let mut out = String::from("🔍 Checking Japanese terms...\n");
    let mut total = 0;
    let mut failure = None;
    for (path, blob) in &files {
        // pathspec が拡張子で選んでいる
        let syntax = report::syntax_of(path).expect("pathspec の拡張子");
        // 読めないブロブも、それまでの報告を出してから検査できなかったとする
        let checked = git(Some(&top), &["cat-file", "blob", blob]).and_then(|blob| {
            let text = String::from_utf8_lossy(&blob).into_owned();
            report::check(dictionary, &text, syntax).map_err(|e| Failure(e.to_string()))
        });
        match checked {
            Ok(reports) => {
                for r in &reports {
                    out.push_str(&format!(
                        "{path}:{}:{}: {} [Error/prh]\n",
                        r.line, r.column, r.message
                    ));
                }
                total += reports.len();
            }
            Err(Failure(why)) => {
                failure = Some(format!("{path}: {why}"));
                break;
            }
        }
    }
    if let Some(why) = failure {
        out.push_str(&format!("❌ 語の検査が失敗しました（{why}）\n"));
        print(&out)?;
        return Ok(2);
    }
    if total == 0 {
        print(&out)?;
        return Ok(0);
    }
    let plural = if total == 1 { "" } else { "s" };
    out.push_str(&format!("\n{total} problem{plural}\n"));
    out.push_str(
        "❌ 崩れた日本語の語があります。上の「誤り => 正しい語」に直してください（辞書: claude-harness の modules/home/parts/ja-terms/prh.yml）\n",
    );
    print(&out)?;
    Ok(1)
}
