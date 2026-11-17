//! `worktree-add.sh <branch> [--from <ref>] [--carry-changes]`: 命名の規則に従って worktree を作る。
//!
//! 作成先はメインの worktree の親のディレクトリの `<repo>-<branch>`（ブランチ名の / は - にする）。ブランチの状態は
//! 自動で見分ける:
//! - ローカルにある → そのままチェックアウトする
//! - リモートにだけある → 追跡ブランチを作ってチェックアウトする
//! - どこにも無い → 既定のブランチ（origin/HEAD → origin/main → origin/master → main → master の最初にあるもの。
//!   git.rs の default_branch）から上流なしで作る。今の HEAD から作ると、作業中のブランチの未完了のコミットが別の
//!   トピックに入るため
//!
//! --from <ref> は新しく作る起点を明示する（今のブランチの続きなら --from HEAD）。既存のブランチに付けたらエラー。
//! --carry-changes は未コミットの変更（index の変更と未追跡のファイルを含む）を patch にして worktree の側へ移し、
//! 元の作業ツリーから取り除く（git-commit のスキルの「mainブランチからの退避」）。stash は全 worktree と全セッションで共有
//! なので使わない。worktree に当てるのに失敗したら、元の変更を残して止める。
//!
//! 共有（.worktreelink）: 新しい worktree の直下に .worktreelink があれば、1 行 1 パス（直下からの相対のパス。空行と
//! # で始まる行は読み飛ばし、前後に空白のある行はエラー）を、メインの worktree の同じパスへの絶対パスの symlink として
//! 作る。git で追跡しない大きなデータ（キャッシュ、参照のデータ）を worktree の間でコピーせずに共有するため。
//! - パスはリテラルだけ。絶対パス、. / .. を含むもの、ワイルドカード（* ? [）と \、! か : で始まるものはエラー
//! - 同じパスや入れ子のパス、親が新しい worktree でディレクトリでない（ファイルか symlink）パスはエラー
//! - メインの worktree で追跡されているパスはエラー（新しい worktree での書き込みがメインの変更になる）
//! - 新しい worktree で git に無視されていなければエラー（symlink が未追跡のファイルに見える）。symlink はディレクトリ
//!   ではないので、`dir/` のようにディレクトリだけに一致するパターンでは無視されない
//! - メインの worktree にそのパスが無ければ、作らずに NOTE で知らせる
//! - エラーのときは symlink を 1 つも作らずに止める（worktree は作ったまま残し、場所を知らせる。直すのは .worktreelink
//!   か .gitignore で、そのブランチの worktree で直せるように）。作成先があり再実行できないので、直す順と、各パスを
//!   張る mkdir -p と ln -s のコマンドを実際のパスで出す
//! - Windows の symlink は開発者モードか管理者の権限が要る。作れなければ、作った symlink を消して止める
//!
//! GitHub のリモートがあり、ローカルにブランチが無く --from も無ければ、ブランチの状態を調べる前に fetch する。古い
//! 追跡ブランチから作ると、ほかのセッションが既定のブランチに入れた変更を取り違える。fetch に失敗したら古い起点で
//! 進まずに止める。
//!
//! 出力: `WORKTREE: <path>`、`BRANCH: <branch>`、`CREATED_BRANCH: yes|no`、`BASE: <ref>`（新しく作ったときだけ）、
//! `SHARED: <path>`（.worktreelink で symlink にしたパスごと）、`NOTE: ...`（GitHub のリモートが複数あるとき、
//! .worktreelink のパスがメインの worktree に無いとき）

use crate::git::git;
use std::path::{Path, PathBuf};
use std::process::Command;

const USAGE: &str = "Usage: worktree-add.sh <branch> [--from <ref>] [--carry-changes]";

fn usage() -> i32 {
    eprintln!("{USAGE}");
    1
}

/// git をそのまま（標準入出力を引き継いで）起動し、成否を返す
fn git_inherit(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .is_ok_and(|s| s.success())
}

pub fn main(args: &[String]) -> i32 {
    let mut branch: Option<&str> = None;
    let mut carry = false;
    let mut from: Option<&str> = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--carry-changes" => carry = true,
            "--from" => match rest.next() {
                Some(v) => from = Some(v),
                None => return usage(),
            },
            a if a.starts_with('-') => return usage(),
            a => {
                if branch.is_some() {
                    return usage();
                }
                branch = Some(a);
            }
        }
    }
    let Some(branch) = branch.filter(|b| !b.is_empty()) else {
        return usage();
    };
    let from = from.filter(|f| !f.is_empty());
    let here = Path::new(".");
    if git(here, &["rev-parse", "--git-dir"]).is_none() {
        for notice in crate::output::take_notices() {
            eprintln!("{notice}");
        }
        eprintln!("ERROR: git リポジトリではありません");
        return 1;
    }

    // メインの worktree（一覧の先頭）を基準にする。linked worktree やサブディレクトリから実行しても同じ場所に作る
    let Some(main_root) = git(here, &["worktree", "list", "--porcelain"]).and_then(|list| {
        list.lines()
            .next()
            .and_then(|l| l.strip_prefix("worktree "))
            .map(PathBuf::from)
    }) else {
        eprintln!("ERROR: メインの worktree を見つけられません");
        return 1;
    };
    // git の出すパスは Windows でも / で区切る。join で \\ を混ぜると、出力の WORKTREE を Git Bash に渡したときに
    // \\ が消える（tests/common の sh_path と同じ理由）ので、/ でつなぐ
    let main_str = main_root.to_string_lossy().into_owned();
    let (parent, repo_name) = main_str.rsplit_once('/').unwrap_or(("", &main_str));
    let dir = PathBuf::from(format!("{parent}/{repo_name}-{}", branch.replace('/', "-")));
    if dir.exists() {
        eprintln!("ERROR: 作成先が既に存在します: {}", dir.display());
        return 1;
    }

    let has_local = |b: &str| {
        git(
            here,
            &[
                "show-ref",
                "--verify",
                "--quiet",
                &format!("refs/heads/{b}"),
            ],
        )
        .is_some()
    };
    let remotes = git(here, &["remote", "-v"]).unwrap_or_default();
    // 起点を使うとき（新しく作る）か、リモートのブランチを探すときだけ要る。origin を明示する（引数なしの fetch は今の
    // ブランチの上流のリモートへ行き、起点の origin/* が更新されない）
    if from.is_none()
        && !has_local(branch)
        && remotes.contains("github.com")
        && git(here, &["remote", "get-url", "origin"]).is_some()
        && !git_inherit(here, &["fetch", "-q", "origin"])
    {
        eprintln!(
            "ERROR: fetch に失敗しました。古い追跡ブランチから作らないよう止めます。リモートへの接続を確かめてください。オフラインなら、ローカルにあるブランチを渡すか、--from <ref> で手元の起点から作ってください"
        );
        return 1;
    }

    let local = has_local(branch);
    let remote_ref = if local {
        None
    } else {
        git(
            here,
            &[
                "for-each-ref",
                "--format=%(refname:short)",
                &format!("refs/remotes/*/{branch}"),
            ],
        )
        .and_then(|refs| refs.lines().next().map(str::to_string))
        .filter(|r| !r.is_empty())
    };
    let dir_arg = dir.to_string_lossy().into_owned();
    let mut base: Option<String> = None;
    let add_args: Vec<String> = if local || remote_ref.is_some() {
        if from.is_some() {
            eprintln!(
                "ERROR: ブランチ {branch} は既にあります。--from は新しいブランチにだけ指定できます"
            );
            return 1;
        }
        match &remote_ref {
            Some(r) => vec![
                dir_arg,
                "--track".into(),
                "-b".into(),
                branch.into(),
                r.clone(),
            ],
            None => vec![dir_arg, branch.into()],
        }
    } else {
        let start = match from {
            Some(f) => f.to_string(),
            None => match crate::git::default_branch(here) {
                Some(d) => d,
                None => {
                    eprintln!(
                        "ERROR: 既定ブランチ（origin/HEAD, origin/main, origin/master, main, master）が見つかりません。--from <ref> で起点を指定してください"
                    );
                    return 1;
                }
            },
        };
        if git(
            here,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{start}^{{commit}}"),
            ],
        )
        .is_none()
        {
            eprintln!("ERROR: 起点のコミットが見つかりません: {start}");
            return 1;
        }
        base = Some(start.clone());
        // 起点がリモートの追跡ブランチでも上流にしない（上流が origin/main だと push 先が main になる）
        vec![
            dir_arg,
            "--no-track".into(),
            "-b".into(),
            branch.into(),
            start,
        ]
    };

    let mut worktree_add = vec!["worktree", "add", "-q"];
    worktree_add.extend(add_args.iter().map(String::as_str));
    if !git_inherit(here, &worktree_add) {
        eprintln!("ERROR: git worktree add が失敗しました");
        return 1;
    }
    if carry && let Err(e) = carry_changes(here, &dir, || {}) {
        eprintln!(
            "ERROR: 変更を worktree に移せませんでした（{e}）。worktree は {} に作ってあります",
            dir.display()
        );
        return 1;
    }

    println!("WORKTREE: {}", dir.display());
    println!("BRANCH: {branch}");
    println!(
        "CREATED_BRANCH: {}",
        if base.is_some() { "yes" } else { "no" }
    );
    if let Some(base) = &base {
        println!("BASE: {base}");
    }
    // fork の運用（GitHub のリモートが複数）では、gh の解決先が worktree に引き継がれないことがある
    let github_remotes: std::collections::BTreeSet<&str> = remotes
        .lines()
        .filter(|l| l.contains("github.com") && l.contains("(fetch)"))
        .filter_map(|l| l.split_whitespace().next())
        .collect();
    if github_remotes.len() >= 2 {
        println!(
            "NOTE: GitHub リモートが複数あります。worktree で gh pr 系コマンドを使う前に gh repo set-default --view で解決先を確認してください"
        );
    }

    // 設定はチェックアウトしたブランチのもの（ブランチごとに .gitignore と揃っている）を読む
    match std::fs::read_to_string(dir.join(".worktreelink")) {
        Ok(text) => share(&dir, &main_root, &text, carry),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
        // 読めない（UTF-8 でない、権限が無い）のを「無い」と同じにすると、共有しなかったことに気付けない
        Err(e) => {
            eprintln!(
                "ERROR: .worktreelink を読めません（{e}）。symlink は作っていません。worktree は {} に作ってあります",
                dir.display()
            );
            1
        }
    }
}

/// 未コミットの変更（index の変更と未追跡ファイルを含む）を from の作業ツリーから dir の作業ツリーへ移す。
///
/// 変更は、from の index の複製に `git add -A` した上で HEAD との差分（`--binary`）にする。本物の index と作業ツリーは
/// 触らず、削除、改名、実行権限、バイナリ、未追跡ファイルを 1 つの patch に集められる。dir に当てるのに成功した後でだけ、
/// from から変更を取り除く（失敗したら元に残す）。stash は全 worktree と全セッションで共有で、push と pop の間に
/// 別のセッションが積むと取り違えるので使わない。元から取り除くのは patch が触れたパスだけで、patch の作成後に元が
/// 変わっていたら取り除かずに止める。after_apply は、dir に当てた後で元を取り除く前に呼ぶ（テストが
/// 並行する編集を差し込むため）。dir の index には載せず作業ツリーの変更にする（ステージ済みかどうかは
/// 引き継がない）
fn carry_changes(from: &Path, dir: &Path, after_apply: impl FnOnce()) -> Result<(), String> {
    // patch のパスはリポジトリのルート基準なので、サブディレクトリから呼ばれてもルートで扱う
    let from = &git(from, &["rev-parse", "--show-toplevel"])
        .map(PathBuf::from)
        .ok_or_else(|| {
            "リポジトリのルートを調べられません（変更は元の作業ツリーに残っています）".to_string()
        })?;
    if git(from, &["status", "--porcelain"]).is_none_or(|s| s.is_empty()) {
        return Ok(());
    }
    let git_path = |name: &str| {
        git(from, &["rev-parse", "--git-path", name])
            .map(|p| std::path::absolute(from.join(p)).unwrap_or_default())
            .ok_or_else(|| "git のディレクトリを調べられません".to_string())
    };
    let index = git_path("index")?;
    let temp_index = git_path(&format!("worktree-add-carry-index-{}", std::process::id()))?;
    let snapshot = || snapshot(from, &index, &temp_index);
    let before = snapshot()
        .map_err(|e| format!("差分を作れません: {e}（変更は元の作業ツリーに残っています）"))?;
    if before.patch.is_empty() {
        return Ok(());
    }
    let applied =
        crate::git::run(dir, &["apply", "--binary"], Some(&before.patch)).ok_or_else(|| {
            "git apply を起動できません（変更は元の作業ツリーに残っています）".to_string()
        })?;
    if !applied.status.success() {
        return Err(format!(
            "git apply が失敗しました: {}（変更は元の作業ツリーに残っています）",
            String::from_utf8_lossy(&applied.stderr).trim()
        ));
    }
    after_apply();
    // patch を作った後に別のセッションが元を編集していたら、その編集は patch に無いので消さない
    if snapshot().map_err(|e| {
        format!("差分を作り直せません: {e}（worktree には当てた後です。変更は両方にあります）")
    })? != before
    {
        return Err(
            "patch を作った後に元の作業ツリーが変わったので、取り除きませんでした（worktree には当てた後です。変更は両方にあります）"
                .to_string(),
        );
    }
    restore_paths(from, &before.paths).map_err(|e| {
        format!("worktree には移しましたが、元の作業ツリーから取り除けませんでした（{e}。変更は両方にあります）")
    })
}

/// 元の作業ツリーの状態の控え。patch と、patch が触れるパス、`git status` の出力
#[derive(PartialEq)]
struct PatchPath {
    /// パスのバイト列（非 UTF-8 のこともある）
    bytes: Vec<u8>,
    /// HEAD に無い（patch が追加するファイル）
    new: bool,
}

#[derive(PartialEq)]
struct Snapshot {
    patch: Vec<u8>,
    paths: Vec<PatchPath>,
    status: Vec<u8>,
}

fn snapshot(from: &Path, index: &Path, temp_index: &Path) -> Result<Snapshot, String> {
    std::fs::copy(index, temp_index).map_err(|e| format!("index を複製できません: {e}"))?;
    let result = (|| {
        let output = |args: &[&str], temp: bool| {
            let mut cmd = Command::new("git");
            cmd.args(args).current_dir(from);
            if temp {
                cmd.env("GIT_INDEX_FILE", temp_index);
            }
            let o = cmd
                .output()
                .map_err(|e| format!("git を起動できません: {e}"))?;
            if o.status.success() {
                Ok(o.stdout)
            } else {
                Err(String::from_utf8_lossy(&o.stderr).trim().to_string())
            }
        };
        output(&["add", "-A"], true)?;
        let patch = output(
            &["diff", "--cached", "--binary", "--no-ext-diff", "HEAD"],
            true,
        )?;
        let names = output(
            &[
                "diff",
                "--cached",
                "--name-status",
                "-z",
                "--no-renames",
                "HEAD",
            ],
            true,
        )?;
        let status = output(&["status", "--porcelain=v1", "-z"], false)?;
        // 出力は「状態 NUL パス NUL」の繰り返し（--no-renames なのでパスは 1 つ）。HEAD にあるかは、パスの
        // 文字列を git に渡して調べず、ここで A（追加）かどうかから決める（非 UTF-8 のパスを壊さない）
        let mut fields = names.split(|b| *b == 0).filter(|f| !f.is_empty());
        let mut paths = Vec::new();
        while let (Some(state), Some(bytes)) = (fields.next(), fields.next()) {
            paths.push(PatchPath {
                bytes: bytes.to_vec(),
                new: state == b"A",
            });
        }
        Ok(Snapshot {
            patch,
            paths,
            status,
        })
    })();
    let _ = std::fs::remove_file(temp_index);
    result
}

/// patch が触れたパスだけを HEAD の状態に戻す（HEAD にあるものは checkout、無いもの（新規ファイル）は削除）。
/// `reset --hard` と `clean -fd` は、patch に入らなかった未追跡ファイル（`git add -A` で拾えない、同じ変更で
/// .gitignore に加えたものなど）も消すので使わない
fn restore_paths(from: &Path, paths: &[PatchPath]) -> Result<(), String> {
    let git_with_paths = |args: &[&str], list: &[&PatchPath]| {
        let spec: Vec<u8> = list
            .iter()
            .flat_map(|p| p.bytes.iter().copied().chain([0]))
            .collect();
        let mut full = vec!["--literal-pathspecs"];
        full.extend_from_slice(args);
        full.extend(["--pathspec-from-file=-", "--pathspec-file-nul"]);
        crate::git::run(from, &full, Some(&spec)).is_some_and(|o| o.status.success())
    };
    let all: Vec<&PatchPath> = paths.iter().collect();
    if !git_with_paths(&["reset", "-q", "HEAD"], &all) {
        return Err("git reset が失敗しました".to_string());
    }
    let (new, in_head): (Vec<&PatchPath>, Vec<&PatchPath>) = all.into_iter().partition(|p| p.new);
    // 新規ファイルの削除は checkout より先にする。HEAD のファイル d を作業ツリーでディレクトリ d/ に置き換えた
    // ときは、checkout が先に d/f を消してしまい、後の remove_file が失敗する
    for p in new {
        let path = from.join(os_path(&p.bytes));
        std::fs::remove_file(&path).map_err(|e| {
            if path.is_dir() {
                format!(
                    "{} はディレクトリです（埋め込みのリポジトリなどは自動では取り除けません。手で消してください）: {e}",
                    path.display()
                )
            } else {
                format!("{}: {e}", path.display())
            }
        })?;
        // 空になったディレクトリは片付ける（空でなければ remove_dir が失敗して止まる）
        let mut dir = path.parent();
        while let Some(d) = dir.filter(|d| *d != from) {
            if std::fs::remove_dir(d).is_err() {
                break;
            }
            dir = d.parent();
        }
    }
    if !in_head.is_empty() && !git_with_paths(&["checkout", "HEAD"], &in_head) {
        return Err("git checkout が失敗しました".to_string());
    }
    Ok(())
}

#[cfg(unix)]
fn os_path(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn os_path(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

/// .worktreelink のパスを、メインの worktree への symlink にする。carry は変更を持ち込んだか（誤りのときに知らせる）
fn share(dir: &Path, main_root: &Path, text: &str, carry: bool) -> i32 {
    let mut paths: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for raw in text.lines() {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        if raw.trim().is_empty() || raw.starts_with('#') {
            continue;
        }
        // 空白は名前の一部になりうるので、落とさずに止める
        if raw.starts_with(char::is_whitespace) || raw.ends_with(char::is_whitespace) {
            errors.push(format!("前後に空白があります: '{raw}'"));
            continue;
        }
        // 末尾の / は symlink の名前には要らない（references/ と references は同じ）
        let line = raw.trim_end_matches('/').to_string();
        // git は引数を pathspec として読む（check-ignore は GIT_LITERAL_PATHSPECS を受け付けない）ので、魔法（:foo、
        // :(top)foo）、ワイルドカード、エスケープになる文字を止め、symlink の名前そのものを判定させる
        if line.is_empty()
            || line.starts_with('/')
            || line.starts_with('!')
            || line.starts_with(':')
            || line.contains(['*', '?', '[', '\\'])
        {
            errors.push(format!("リテラルの相対パスではありません: {line}"));
            continue;
        }
        let wrapped = format!("/{line}/");
        if wrapped.contains("/./") || wrapped.contains("/../") || wrapped.contains("//") {
            errors.push(format!(". / .. や空の要素を含むパスは使えません: {line}"));
            continue;
        }
        // 同じパスや入れ子のパスは、後の symlink が先の symlink をたどってメインの worktree の中に作ってしまう
        let overlap = seen.iter().any(|s| {
            line == *s || line.starts_with(&format!("{s}/")) || s.starts_with(&format!("{line}/"))
        });
        seen.push(line.clone());
        if overlap {
            errors.push(format!("重なるパスがあります: {line}"));
            continue;
        }
        // 親は無いか、symlink でないディレクトリであること（作る途中で落ちたり、symlink をたどってメインの worktree の
        // 中に作ったりしないように）
        let parts: Vec<&str> = line.split('/').collect();
        let mut ancestor = dir.to_path_buf();
        let mut parent_ok = true;
        for part in &parts[..parts.len() - 1] {
            ancestor.push(part);
            match std::fs::symlink_metadata(&ancestor) {
                Ok(m) if m.file_type().is_symlink() || !m.is_dir() => {
                    parent_ok = false;
                    break;
                }
                _ => {}
            }
        }
        if !parent_ok {
            errors.push(format!(
                "親が新しい worktree でディレクトリではありません: {line}"
            ));
            continue;
        }
        if std::fs::symlink_metadata(dir.join(&line)).is_ok() {
            errors.push(format!(
                "新しい worktree に既にあります（追跡されているか、--carry-changes で持ち込んだ）: {line}"
            ));
            continue;
        }
        match crate::git::run(dir, &["check-ignore", "-q", "--", &line], None) {
            Some(out) if out.status.success() => {}
            Some(out) if out.status.code() == Some(1) => {
                errors.push(format!(
                    "git に無視されていません（symlink が未追跡ファイルに見える。dir/ でなく dir か /dir と書く）: {line}"
                ));
                continue;
            }
            other => {
                let (code, err) = other.map_or(("なし".to_string(), String::new()), |o| {
                    (
                        o.status
                            .code()
                            .map_or("なし".to_string(), |c| c.to_string()),
                        String::from_utf8_lossy(&o.stderr).trim().to_string(),
                    )
                });
                errors.push(format!(
                    "git check-ignore が失敗しました（終了コード {code}: {err}）: {line}"
                ));
                continue;
            }
        }
        // メインのブランチが追跡するパスに張ると、新しい worktree での書き込みがメインの変更になる
        if git(main_root, &["ls-files", "--", &line]).is_some_and(|s| !s.is_empty()) {
            errors.push(format!("メイン worktree で git に追跡されています: {line}"));
            continue;
        }
        paths.push(line);
    }
    if !errors.is_empty() {
        eprintln!("ERROR: .worktreelink に使えないパスがあります。symlink は作っていません:");
        for e in &errors {
            eprintln!("  {e}");
        }
        eprintln!(
            "worktree は {} に作ってあります{}。作成先があるのでスクリプトは再実行できません。次の順に直してください:",
            dir.display(),
            if carry {
                "（--carry-changes の変更は worktree に移してあります）"
            } else {
                ""
            }
        );
        eprintln!(
            "1. その worktree（そのブランチ）で .worktreelink か .gitignore を直す（書き方は worktree-add.sh の冒頭のコメント）"
        );
        eprintln!("2. .worktreelink の各パスを、メイン worktree への絶対パスの symlink にする:");
        let quote = super::wait_step::quote;
        let (wt, main) = (dir.to_string_lossy(), main_root.to_string_lossy());
        // 張るのは link_all と同じく、メイン worktree にあるパスだけ（無いパスに張ると壊れた symlink になる）
        for path in &paths {
            let target = format!("{main}/{path}");
            if !Path::new(&target).exists() {
                eprintln!(
                    "  （メイン worktree に {path} が無いので張らない。用意してから下の形で張る）"
                );
                continue;
            }
            let parent = path
                .rsplit_once('/')
                .map_or(wt.to_string(), |(p, _)| format!("{wt}/{p}"));
            eprintln!(
                "  mkdir -p {} && ln -s {} {}",
                quote(&parent),
                quote(&target),
                quote(&format!("{wt}/{path}"))
            );
        }
        let (wt_q, main_q) = (quote(&wt), quote(&main));
        eprintln!(
            "  mkdir -p \"$(dirname {wt_q}/<直したパス>)\" && ln -s {main_q}/<直したパス> {wt_q}/<直したパス>"
        );
        return 1;
    }

    match link_all(dir, main_root, &paths, symlink) {
        Ok((linked, notes)) => {
            for note in notes {
                println!("{note}");
            }
            for l in linked {
                println!("SHARED: {l}");
            }
            0
        }
        Err((path, e)) => {
            eprintln!(
                "ERROR: {path} の symlink を作れませんでした（{e}。Windows は開発者モードか管理者の権限が要る）。それまでに作った symlink は消しました（消せなかったものは上に出した）。worktree は {} に作ってあります",
                dir.display()
            );
            1
        }
    }
}

/// 作った symlink のパスと NOTE の行
type Linked<'a> = (Vec<&'a String>, Vec<String>);

/// paths をメインの worktree への symlink にする（make で作る）。メインの worktree に無いパスは作らずに NOTE にする。
/// 途中で作れなかったら、作った symlink を消してから、作れなかったパスと理由を返す（1 つも残さない）
fn link_all<'a>(
    dir: &Path,
    main_root: &Path,
    paths: &'a [String],
    make: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<Linked<'a>, (&'a String, std::io::Error)> {
    let mut linked: Vec<&String> = Vec::new();
    let mut notes = Vec::new();
    for path in paths {
        // / でつなぐ（Windows でも git の出すパスと区切りを揃える）
        let target = PathBuf::from(format!("{}/{path}", main_root.to_string_lossy()));
        if !target.exists() {
            let link = format!("{}/{path}", dir.to_string_lossy());
            notes.push(format!(
                "NOTE: メイン worktree に {path} がないため共有しません（{}）。必要ならメイン worktree に用意してから、ln -s {} {} で張る",
                target.display(),
                super::wait_step::quote(&target.to_string_lossy()),
                super::wait_step::quote(&link)
            ));
            continue;
        }
        let link = dir.join(path);
        let made = link
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| make(&target, &link));
        if let Err(e) = made {
            for l in &linked {
                // Windows のディレクトリの symlink は remove_dir でしか消えない
                let link = dir.join(l);
                if let Err(e) = std::fs::remove_file(&link).or_else(|_| std::fs::remove_dir(&link))
                {
                    eprintln!(
                        "ERROR: 作った {l} の symlink を消せませんでした（{e}）。手で消してください"
                    );
                }
            }
            return Err((path, e));
        }
        linked.push(path);
    }
    Ok((linked, notes))
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// Windows の symlink はディレクトリとファイルで作り方が違う
#[cfg(windows)]
fn symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", dir.join("no-global-config"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}");
    }

    fn carry_fixture(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("worktree-add-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (from, dir) = (root.join("app"), root.join("app-x"));
        std::fs::create_dir_all(&from).unwrap();
        sh(&from, &["init", "-q", "-b", "main"]);
        (root, from, dir)
    }

    fn commit_and_branch(from: &Path, dir: &Path) {
        sh(from, &["add", "-A"]);
        sh(from, &["commit", "-q", "-m", "init"]);
        sh(
            from,
            &["worktree", "add", "-q", dir.to_str().unwrap(), "-b", "x"],
        );
    }

    #[test]
    fn a_file_replaced_by_a_directory_is_carried_and_the_source_restored() {
        let (root, from, dir) = carry_fixture("file-to-dir");
        std::fs::write(from.join("d"), "file\n").unwrap();
        commit_and_branch(&from, &dir);
        std::fs::remove_file(from.join("d")).unwrap();
        std::fs::create_dir(from.join("d")).unwrap();
        std::fs::write(from.join("d/f"), "new\n").unwrap();
        let result = carry_changes(&from, &dir, || {});
        assert_eq!(result, Ok(()));
        assert_eq!(
            std::fs::read_to_string(from.join("d")).unwrap().trim_end(),
            "file"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("d/f")).unwrap().trim_end(),
            "new"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_are_carried_and_removed_from_the_source() {
        use std::os::unix::ffi::OsStrExt;
        let (root, from, dir) = carry_fixture("non-utf8");
        let tracked = std::ffi::OsStr::from_bytes(b"tracked\xff");
        let added = std::ffi::OsStr::from_bytes(b"bad\xff");
        std::fs::write(from.join(tracked), "base\n").unwrap();
        commit_and_branch(&from, &dir);
        std::fs::write(from.join(tracked), "edited\n").unwrap();
        std::fs::write(from.join(added), "new\n").unwrap();
        let result = carry_changes(&from, &dir, || {});
        assert_eq!(result, Ok(()));
        assert_eq!(std::fs::read(from.join(tracked)).unwrap(), b"base\n");
        assert!(!from.join(added).exists());
        assert_eq!(std::fs::read(dir.join(tracked)).unwrap(), b"edited\n");
        assert_eq!(std::fs::read(dir.join(added)).unwrap(), b"new\n");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_edit_made_after_the_patch_keeps_the_source_untouched() {
        let root = std::env::temp_dir().join(format!("worktree-add-race-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (from, dir) = (root.join("app"), root.join("app-x"));
        std::fs::create_dir_all(&from).unwrap();
        sh(&from, &["init", "-q", "-b", "main"]);
        std::fs::write(from.join("a.txt"), "base\n").unwrap();
        sh(&from, &["add", "-A"]);
        sh(&from, &["commit", "-q", "-m", "init"]);
        sh(
            &from,
            &["worktree", "add", "-q", dir.to_str().unwrap(), "-b", "x"],
        );
        std::fs::write(from.join("a.txt"), "carried\n").unwrap();
        let result = carry_changes(&from, &dir, || {
            std::fs::write(from.join("a.txt"), "edited by another session\n").unwrap();
            std::fs::write(from.join("late.txt"), "late\n").unwrap();
        });
        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(from.join("a.txt")).unwrap(),
            "edited by another session\n"
        );
        assert!(from.join("late.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_link_that_cannot_be_made_removes_the_links_made_before_it() {
        let root =
            std::env::temp_dir().join(format!("worktree-add-link-all-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (main_root, dir) = (root.join("app"), root.join("app-x"));
        for p in ["one", "two"] {
            std::fs::create_dir_all(main_root.join(p)).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        let paths = vec!["one".to_string(), "two".to_string()];
        let result = link_all(&dir, &main_root, &paths, |target, link| {
            if link.ends_with("two") {
                return Err(std::io::Error::other("injected"));
            }
            symlink(target, link)
        });
        let Err((path, _)) = result else {
            panic!("two で失敗する");
        };
        assert_eq!(path, "two");
        assert!(
            std::fs::symlink_metadata(dir.join("one")).is_err(),
            "one の symlink が残る"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
