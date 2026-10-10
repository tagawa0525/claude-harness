//! git の呼び出し。PATH の git を使う（テストは PATH を差し替えて挙動を作る）

use std::path::Path;
use std::process::Command;

/// `git <args>` を dir で実行し、成功なら標準出力（末尾改行を除く）を返す。
/// 失敗（非 0 終了、dir が無い、git が無い）は None
pub fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
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

/// 自分のプロジェクトか（自分の規約を当てるか）。`git config claude-hooks.own-project` の値に従い、
/// 未設定なら判定して書き込む（一度だけ判定し、手で書いた値を優先する）。
/// 他人のプロジェクト = upstream リモートを持つ fork、または GitHub のリモートの所有者に
/// `git config claude-hooks.owner`（自分のアカウント。組織など複数可）以外がいる。
/// GitHub のリモートが無ければ自分のもの。自分のアカウントが未設定なら所有者で判定できないので、
/// 書き込まずにその都度判定する（設定した後で判定し直せるように）
pub fn own_project(dir: &Path) -> bool {
    match git(
        dir,
        &["config", "--type=bool", "--get", "claude-hooks.own-project"],
    )
    .as_deref()
    {
        Some("true") => return true,
        Some("false") => return false,
        _ => {}
    }
    let owners: Vec<String> = git(dir, &["config", "--get-all", "claude-hooks.owner"])
        .map(|s| s.lines().map(str::to_string).collect())
        .unwrap_or_default();
    let own = !is_fork(dir) && !has_foreign_github_remote(dir, &owners);
    if !owners.is_empty() {
        let value = if own { "true" } else { "false" };
        let _ = git(
            dir,
            &["config", "--local", "claude-hooks.own-project", value],
        );
    }
    own
}

/// GitHub のリモートの所有者に、自分のアカウント以外がいるか。アカウントが無ければ判定しない。
/// SSH の別名（git@github-work:…）のように github.com を含まないリモートは見ない
fn has_foreign_github_remote(dir: &Path, owners: &[String]) -> bool {
    if owners.is_empty() {
        return false;
    }
    let remotes = git(dir, &["remote", "-v"]).unwrap_or_default();
    let owner = regex::Regex::new(r"github\.com[:/]([^/\s]+)/").expect("固定の正規表現");
    owner
        .captures_iter(&remotes)
        .any(|c| !owners.iter().any(|o| o.eq_ignore_ascii_case(&c[1])))
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
