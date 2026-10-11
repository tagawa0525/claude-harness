//! git の呼び出し。PATH の git を使う（テストは PATH を差し替えて挙動を作る）

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

/// `git <args>` を dir で実行する（input があれば標準入力に渡す）。dir が無いか git を起動できなければ None。
/// dir があるのに起動できない（PATH に git が無い、dir に入れない）のは環境の問題なので、理由を利用者に
/// 見せる（[`crate::output::notice`]。理由はプロセスに 1 つの置き場に集まるので、結合テストで確かめる）。起動した後の失敗（入力を書けない、待てない）は、非 0 の終了と同じく呼ぶ側に任せる
pub fn run(dir: &Path, args: &[&str], input: Option<&[u8]>) -> Option<Output> {
    // まだ無いディレクトリは、判定するコマンドが作る前の場所（mkdir の後の cd や -C）を指すことがある。
    // is_dir は読めない（親に入れない）ときも false なので使わず、無いときだけ黙って返す
    match std::fs::metadata(dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Ok(meta) if !meta.is_dir() => return None,
        _ => {}
    }
    let mut child = match Command::new("git")
        .args(args)
        .current_dir(dir)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            crate::output::notice(format!(
                "claude-hooks: git を起動できません（PATH に git が無いか、{} に入れません）: {e}",
                dir.display()
            ));
            return None;
        }
    };
    // 入力は別のスレッドで書く（書き終える前に git が出力でパイプを満たすと、互いに待って止まる）。
    // 書けなくても（git が先に終わった）、終了の状態は待って受ける
    let stdin = child.stdin.take();
    std::thread::scope(|scope| {
        if let (Some(input), Some(mut stdin)) = (input, stdin) {
            scope.spawn(move || {
                let _ = stdin.write_all(input);
            });
        }
        child.wait_with_output().ok()
    })
}

/// `git <args>` を dir で実行し、成功なら標準出力（末尾改行を除く）を返す。非 0 の終了と、[`run`] が
/// None を返すときは None
pub fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = run(dir, args, None)?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string(),
    )
}

/// リモートのいずれかが github.com を指すか（PR フローの適用対象か）
pub fn has_github_remote(dir: &Path) -> bool {
    git(dir, &["remote", "-v"]).is_some_and(|s| s.contains("github.com"))
}

/// 他人のプロジェクトの fork（上流に PR を出す worktree）か。`upstream` リモートの有無で判定する。
/// git の pre-commit / commit-msg hook（modules/home/parts/git.nix）と同じ判定
pub fn is_fork(dir: &Path) -> bool {
    git(dir, &["remote", "get-url", "upstream"]).is_some()
}

pub fn current_branch(dir: &Path) -> String {
    git(dir, &["branch", "--show-current"]).unwrap_or_default()
}

/// 既定ブランチ。新しいトピックのブランチの起点に使う ref（`origin/main` や `main`）。
/// origin/HEAD → origin/main → origin/master → main → master の最初にあるもの。
/// .claude/scripts/worktree-add.sh と branch-topics.sh も同じ順で探す
pub fn default_branch(dir: &Path) -> Option<String> {
    if let Some(r) = git(
        dir,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    ) {
        return Some(r);
    }
    [
        "refs/remotes/origin/main",
        "refs/remotes/origin/master",
        "refs/heads/main",
        "refs/heads/master",
    ]
    .into_iter()
    .find(|r| git(dir, &["show-ref", "--verify", "--quiet", r]).is_some())
    .map(|r| {
        r.strip_prefix("refs/remotes/")
            .or_else(|| r.strip_prefix("refs/heads/"))
            .unwrap_or(r)
            .to_string()
    })
}

/// 既定ブランチの ref（`origin/main`）をローカルのブランチ名（`main`）にする
pub fn local_name(default_ref: &str) -> &str {
    default_ref.strip_prefix("origin/").unwrap_or(default_ref)
}
