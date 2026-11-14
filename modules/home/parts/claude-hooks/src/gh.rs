//! gh の呼び出し。PATH の gh を使う（テストは偽の gh を PATH 先頭に置き、
//! 引数列 "$*" の前方一致で応答するため、引数の順序と --jq フィルタは bash 版と揃える）

use std::path::Path;
use std::process::Command;

/// `gh <args>` を dir で実行し、成功なら標準出力を返す。失敗は Err。
/// 起動できない（PATH に gh が無い、dir に入れない）のは環境の問題なので、理由を利用者に見せる
/// （[`crate::output::notice`]）。dir が無いかディレクトリでなければ、git と同じく黙って Err
/// （[`crate::git::not_a_directory`]）
pub fn gh(dir: &Path, args: &[&str]) -> Result<String, ()> {
    if crate::git::not_a_directory(dir) {
        return Err(());
    }
    let out = Command::new("gh")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| {
            crate::output::notice(format!(
                "claude-hooks: gh を起動できません（PATH に gh が無いか、{} に入れません）: {e}",
                dir.display()
            ));
        })?;
    if !out.status.success() {
        return Err(());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .trim_end_matches('\n')
        .to_string())
}

/// `gh <args>` を、対象を repo（`<owner>/<repo>`）にして起動し、標準入出力を引き継いで終了コードを返す（スクリプトの
/// 中身。[`crate::script`]）。cwd のリポジトリに依らないよう GH_REPO を渡し、GH_HOST も固定する（gh api は GH_REPO の
/// ホストより環境の GH_HOST を使う。GH_REPO=github.com/<owner>/<repo> でも GH_HOST のホストへつなぐことを 2026-10-12 に
/// 確かめた）。起動できなければ理由を標準エラーに出して 1
pub fn run_for_repo(repo: &str, args: &[&str]) -> i32 {
    match for_repo(repo, args).status() {
        Ok(status) => crate::child::exit_code(status),
        Err(e) => {
            eprintln!("ERROR: gh を起動できません（PATH に gh が無い）: {e}");
            1
        }
    }
}

/// [`run_for_repo`] と同じく repo に向けて起動し、標準出力を受けて返す（標準エラーは引き継ぐ）。失敗すれば、
/// スクリプトの終了コードにする gh の終了コード（起動できなければ理由を標準エラーに出して 1）を Err で返す
pub fn output_for_repo(repo: &str, args: &[&str]) -> Result<Vec<u8>, i32> {
    let out = for_repo(repo, args)
        .stderr(std::process::Stdio::inherit())
        .output()
        .map_err(|e| {
            eprintln!("ERROR: gh を起動できません（PATH に gh が無い）: {e}");
            1
        })?;
    if !out.status.success() {
        return Err(out.status.code().unwrap_or(1));
    }
    Ok(out.stdout)
}

/// [`run_for_repo`] と同じく repo に向けて起動し、標準エラーを捨てて標準出力を受ける。起動できないか失敗すれば
/// None（失敗を待ちの続きとして扱う呼び出し元のため）
pub fn quiet_output_for_repo(repo: &str, args: &[&str]) -> Option<Vec<u8>> {
    let out = for_repo(repo, args)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

/// [`run_for_repo`] と同じく repo に向けて起動し、標準出力と標準エラーを捨てて終了の状態を返す（起動できなければ Err）
pub fn quiet_status_for_repo(
    repo: &str,
    args: &[&str],
) -> std::io::Result<std::process::ExitStatus> {
    for_repo(repo, args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
}

/// [`run_for_repo`] と同じく repo に向けて起動し、標準出力を捨てる。失敗すれば、gh の標準エラー（起動できなければ
/// その理由）を Err で返す（呼び出し元が自分の文の前に出す）
pub fn quiet_stdout_output_for_repo(repo: &str, args: &[&str]) -> Result<(), String> {
    let out = for_repo(repo, args)
        .stdout(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("ERROR: gh を起動できません（PATH に gh が無い）: {e}\n"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

fn for_repo(repo: &str, args: &[&str]) -> Command {
    let mut cmd = Command::new("gh");
    cmd.args(args)
        .env("GH_REPO", repo)
        .env("GH_HOST", "github.com");
    cmd
}
