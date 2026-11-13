//! `post-merge-cleanup.sh <branch>`: PR のマージの後（または /git-merge の後）に、ブランチと worktree を片付ける。
//! 順は gh-pr-merge のスキルの「クリーンアップ」と、`.claude/scripts/post-merge-cleanup.sh` の説明に書く:
//!
//! - （0 の前）cwd のリポジトリに branch の痕跡（ローカルのブランチ、リモート追跡の ref、リモートのブランチ、GitHub の
//!   リモートではそのブランチを head とするマージ済みの PR）が無ければ、何も変えずに止める。確かめられないときも止める
//! - 段階 0: ローカルの branch の先端が既定のブランチ（無ければ、追うリモートの既定のブランチを fetch したもの）の祖先で
//!   なければ、何も消さずに止める
//! - 段階 1: branch をチェックアウトしている worktree を消す（既定のブランチの worktree に移ってから）
//! - 段階 2: 既定のブランチへ切り替え、fetch --prune、pull --ff-only（リモートが無ければ省く）。配置するものが変われば NOTE
//! - 段階 3: origin から予約のタグを取り、既定のブランチの履歴に記録が入った番号のタグを消す（design-records prune）
//! - 段階 4: ローカルのブランチを消す（-d）
//! - 段階 5: リモートのブランチを消す（リモートの既定のブランチが先端を含まないときと、open の PR の head のときは残す）
//!
//! git と gh の失敗のうち、続けられないものは、その標準エラーを出したまま、その終了コードで終える

use std::path::PathBuf;
use std::process::{Command, Output};

const USAGE: &str = "Usage: post-merge-cleanup.sh <branch>";

/// git を起動して出力を受ける（起動できなければ None）
fn run(args: &[&str]) -> Option<Output> {
    Command::new("git").args(args).output().ok()
}

/// git が成功したら、標準出力（末尾の改行を除く）
fn git(args: &[&str]) -> Option<String> {
    run(args).filter(|o| o.status.success()).map(|o| {
        String::from_utf8_lossy(&o.stdout)
            .trim_end_matches('\n')
            .to_string()
    })
}

fn ok(args: &[&str]) -> bool {
    run(args).is_some_and(|o| o.status.success())
}

/// git を標準入出力を引き継いで起動し、失敗すればその終了コード（sh の set -e と同じく、そこで止める）
fn step(args: &[&str]) -> Result<(), i32> {
    match Command::new("git").args(args).status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("ERROR: git を起動できません: {e}");
            Err(1)
        }
    }
}

fn worktree_of(list: &str, branch: &str) -> Option<String> {
    let want = format!("refs/heads/{branch}");
    let mut path = None;
    let mut found = None;
    for line in list.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(p.to_string());
        } else if line.strip_prefix("branch ") == Some(want.as_str()) {
            found = path.clone();
        }
    }
    found
}

/// name のコマンドが PATH にあるか（sh の command -v と同じく、起動はしない。--version などに答えない偽物もある）
fn on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        dir.join(name).is_file() || (cfg!(windows) && dir.join(format!("{name}.exe")).is_file())
    })
}

fn gh_available() -> bool {
    on_path("gh")
}

pub fn main(args: &[String]) -> i32 {
    let [branch] = args else {
        eprintln!("{USAGE}");
        return 1;
    };
    if branch.is_empty() {
        eprintln!("{USAGE}");
        return 1;
    }
    match cleanup(branch) {
        Ok(()) => 0,
        Err(code) => code,
    }
}

fn cleanup(branch: &str) -> Result<(), i32> {
    if git(&["rev-parse", "--git-dir"]).is_none() {
        eprintln!("ERROR: git リポジトリではありません");
        return Err(1);
    }
    let Some(default) = ["main", "master"].into_iter().find(|b| {
        ok(&[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{b}"),
        ])
    }) else {
        eprintln!("ERROR: 既定ブランチ（main / master）が見つかりません");
        return Err(1);
    };
    if branch == default {
        eprintln!("ERROR: 既定ブランチ {default} は削除しません");
        return Err(1);
    }
    // リモートの名前はブランチを消す前に読む（branch -d で設定も消える）
    let remote = git(&["config", &format!("branch.{branch}.remote")])
        .unwrap_or_else(|| "origin".to_string());

    // --- 1. worktree の場所 ---
    let list = git(&["worktree", "list", "--porcelain"]).unwrap_or_default();
    let main_root = list
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("worktree "))
        .unwrap_or("")
        .to_string();
    let wt_path = worktree_of(&list, branch);
    let default_root = worktree_of(&list, default);
    // 以降は既定のブランチの worktree（どこにもチェックアウトされていなければメインの worktree）で行う。今いる
    // worktree が対象のものでも、無関係なブランチのものでも、そこでは既定のブランチへ switch できない
    let base = PathBuf::from(default_root.clone().unwrap_or_else(|| main_root.clone()));
    if let Err(e) = std::env::set_current_dir(&base) {
        eprintln!("ERROR: {} に移れません: {e}", base.display());
        return Err(1);
    }

    trace(branch, &remote, &main_root)?;
    merged(branch, default, default_root.as_deref())?;

    match &wt_path {
        Some(wt) => {
            step(&["worktree", "remove", wt])?;
            println!("WORKTREE_REMOVED: {wt}");
        }
        None => println!("WORKTREE_REMOVED: none"),
    }

    update_default(default)?;
    reservations()?;

    if ok(&[
        "show-ref",
        "--verify",
        "--quiet",
        &format!("refs/heads/{branch}"),
    ]) {
        step(&["branch", "-d", "-q", branch])?;
        println!("LOCAL_BRANCH: deleted");
    } else {
        println!("LOCAL_BRANCH: none");
    }

    remote_branch(branch, &remote, default)
}

/// 手元（ローカルのブランチ、リモート追跡の ref）を先に見て、無いときだけネットワークに行く
fn trace(branch: &str, remote: &str, main_root: &str) -> Result<(), i32> {
    let no_trace = |extra: &str| {
        eprintln!(
            "ERROR: {branch} の痕跡（ローカルのブランチ、worktree、リモート追跡の ref、リモートのブランチ{extra}）が {main_root} にありません。何も変えていません"
        );
        eprintln!(
            "  修正方法: そのブランチのリポジトリの中で実行する（別のリポジトリのブランチなら cd <そのリポジトリ> && post-merge-cleanup.sh {branch}）。ブランチ名を確かめる。このリポジトリで片付けを終えた後のやり直しなら、もう何もしなくてよい"
        );
        Err(1)
    };
    let cannot_check = |why: &str, fix: &str| {
        eprintln!("ERROR: {branch} の痕跡が {main_root} の手元に無く、{why}。何も変えていません");
        eprintln!(
            "  修正方法: {fix}。そのあと、もう一度実行する。別のリポジトリのブランチなら cd <そのリポジトリ> && post-merge-cleanup.sh {branch}"
        );
        Err(1)
    };
    // チェックアウトした worktree はローカルのブランチを持つので、ブランチの有無で数える。リモート追跡の ref は
    // リモートごとに見る（refs/remotes/*/<branch> の glob はブランチ名の / を跨がない）
    let mut found = ok(&[
        "show-ref",
        "--verify",
        "--quiet",
        &format!("refs/heads/{branch}"),
    ]) || git(&["remote"]).unwrap_or_default().lines().any(|r| {
        ok(&[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/remotes/{r}/{branch}"),
        ])
    });
    if !found && ok(&["remote", "get-url", remote]) {
        // 標準エラーは分けて受ける（成功しても警告を出すことがあり、混ぜると痕跡があると読み違える）
        let out = run(&[
            "ls-remote",
            "--heads",
            remote,
            &format!("refs/heads/{branch}"),
        ]);
        match out {
            Some(o) if o.status.success() => found = !o.stdout.is_empty(),
            other => {
                let err = other
                    .map(|o| String::from_utf8_lossy(&o.stderr).replace('\n', " "))
                    .unwrap_or_default();
                return cannot_check(
                    &format!("{remote} のブランチを確かめられない（git ls-remote が失敗: {err}）"),
                    &format!("{remote} に届かない原因（ネットワークや認証）を直す"),
                );
            }
        }
    }
    if found {
        return Ok(());
    }
    if !git(&["remote", "-v"])
        .unwrap_or_default()
        .contains("github.com")
    {
        return no_trace("");
    }
    if !gh_available() {
        return cannot_check(
            "マージ済みの PR を確かめられない（gh が無い）",
            "gh を PATH に置く",
        );
    }
    // gh pr list は新しい順に返す。名前だけで照らすので、別のリポジトリのブランチと同じ名前の古い PR でも通る。どの PR
    // を痕跡にしたかを NOTE で見せ、呼ぶ側が自分の PR かを読めるようにする
    let out = Command::new("gh")
        .args([
            "pr",
            "list",
            "--head",
            branch,
            "--state",
            "merged",
            "--json",
            "url",
            "--jq",
            ".[0].url // empty",
        ])
        .output();
    let merged_pr = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        other => {
            let err = other
                .map(|o| String::from_utf8_lossy(&o.stderr).replace('\n', " "))
                .unwrap_or_default();
            return cannot_check(
                &format!("マージ済みの PR を確かめられない（gh pr list が失敗: {err}）"),
                "gh の失敗の原因（ネットワークや認証）を直す",
            );
        }
    };
    if merged_pr.is_empty() {
        return no_trace("、マージ済みの PR");
    }
    println!(
        "NOTE: {branch} は手元にもリモートにも無く、マージ済みの PR {merged_pr} を痕跡として片付けを続ける。自分の PR でなければ、別のリポジトリのブランチを渡している（cd <そのリポジトリ> && post-merge-cleanup.sh {branch}）"
    );
    Ok(())
}

/// 段階 0: マージ済みか。先端が既定のブランチの祖先か（squash や rebase のマージは、先端が履歴に残らないので未マージの扱い）。
/// gh pr merge の後はローカルの既定のブランチがまだ pull されていないので、追うリモートの既定のブランチも fetch して
/// 見る。refs/heads/ を付けるのは、同じ名前のタグが先に解決されて別のコミットで判定されるのを避けるため
fn merged(branch: &str, default: &str, default_root: Option<&str>) -> Result<(), i32> {
    let local = format!("refs/heads/{branch}");
    if !ok(&["show-ref", "--verify", "--quiet", &local])
        || ok(&[
            "merge-base",
            "--is-ancestor",
            &local,
            &format!("refs/heads/{default}"),
        ])
    {
        return Ok(());
    }
    let upstream = git(&[
        "for-each-ref",
        "--format=%(upstream)",
        &format!("refs/heads/{default}"),
    ])
    .unwrap_or_default();
    let mut fetch_error = String::new();
    if upstream.starts_with("refs/remotes/") {
        let up_remote = git(&["config", &format!("branch.{default}.remote")]).unwrap_or_default();
        match run(&["fetch", "-q", &up_remote]) {
            Some(o) if o.status.success() => {
                if ok(&["merge-base", "--is-ancestor", &local, &upstream]) {
                    return Ok(());
                }
            }
            other => {
                let out = other
                    .map(|o| {
                        let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
                        s.push_str(&String::from_utf8_lossy(&o.stderr));
                        s.trim_end().replace('\n', " ")
                    })
                    .unwrap_or_default();
                fetch_error = format!("{up_remote} の fetch に失敗した（{out}）");
            }
        }
    }
    let short_up = upstream.strip_prefix("refs/remotes/").unwrap_or(&upstream);
    let mut detail = if short_up.is_empty() {
        format!("{default}の履歴にブランチの先端が無い")
    } else {
        format!("{default}、{short_up}の履歴にブランチの先端が無い")
    };
    if !fetch_error.is_empty() {
        detail.push_str(&format!(
            "。{fetch_error}。マージ済みでも、{short_up} を取れないと確かめられない"
        ));
    }
    let mut hint =
        "マージを終える（コンフリクトの解消や hook の拒否の原因を直して、もう一度マージする）"
            .to_string();
    if let Some(root) = default_root
        && ok(&["rev-parse", "-q", "--verify", "MERGE_HEAD"])
    {
        detail.push_str(&format!(
            "。{default} の worktree（{root}）に MERGE_HEAD が残っている（マージが途中で止まっている）"
        ));
        hint.push_str("。やり直すなら、途中のマージを git merge --abort で取り消してから直す");
    }
    if !fetch_error.is_empty() {
        hint = format!("fetch の失敗の原因（ネットワークや認証）を直す。{hint}");
    }
    eprintln!(
        "ERROR: {branch} は {default} にまだマージされていません（{detail}）。何も消していません"
    );
    eprintln!("  修正方法: {hint}。そのあと、もう一度実行する");
    Err(1)
}

/// 段階 2: 既定のブランチを最新にする。hook のバイナリと ~/.claude は rebuild か claude-sync で配置される。pull しただけでは
/// 古いまま動くので知らせる（自動では実行しない。rebuild は sudo が要り、走っている別のセッションの hook も入れ替わる）。
/// 判定は広く取る（配置するものを 1 つずつ挙げると漏れ、多めに知らせても害は無い）
fn update_default(default: &str) -> Result<(), i32> {
    if git(&["branch", "--show-current"]).as_deref() != Some(default) {
        step(&["switch", "-q", default])?;
    }
    // リモートの無いリポジトリ（/git-merge でローカルにマージした後）では最新にする先が無い
    if git(&["remote"]).unwrap_or_default().is_empty() {
        return Ok(());
    }
    step(&["fetch", "-q", "--prune"])?;
    let before = git(&["rev-parse", "HEAD"]).unwrap_or_default();
    step(&["pull", "-q", "--ff-only"])?;
    // ~/.claude の配置の元（ハーネス）は同期のコマンド（claude-setup の crate。古い版の clone と、ハーネスを分ける前の
    // nixfiles は claude-sync.sh）を持ち、rebuild する構成のリポジトリ（nixfiles）は nix-rebuild.sh を持つ
    let has = |path: &str| ok(&["cat-file", "-e", &format!("HEAD:{path}")]);
    let has_sync = has("modules/home/parts/claude-setup/Cargo.toml")
        || has("modules/home/scripts/claude-sync.sh");
    let has_rebuild = has("modules/home/scripts/nix-rebuild.sh");
    let changed = |paths: &[&str]| {
        let mut args = vec!["diff", "--name-only", before.as_str(), "HEAD", "--"];
        args.extend(paths);
        git(&args).is_some_and(|s| !s.is_empty())
    };
    if has_rebuild {
        if changed(&["modules", "hosts", "secrets", "flake.nix", "flake.lock"]) {
            println!(
                "NOTE: rebuild が要る（modules、hosts、secrets か flake が変わった）。nix-rebuild rebuild まで hook や設定は古い版のまま動く"
            );
        }
    } else if has_sync && changed(&["modules", "flake.nix", "flake.lock"]) {
        // ハーネスの hook とモジュールは、取り込む側（nixfiles）の lock が固定した版で配置されるので、lock を進めてから rebuild する
        println!(
            "NOTE: lock の更新と rebuild が要る（modules か flake が変わった）。取り込む側（nixfiles）で nix-rebuild update を実行するまで hook や設定は古い版のまま動く"
        );
    }
    // rebuild する構成のリポジトリだけの .claude はプロジェクトの指示で、その場で読まれる
    if has_sync && changed(&[".claude"]) {
        // 引数なしの claude-sync は既定の clone を読む。pull したのがそこと限らないので、このリポジトリを元に渡す形で示す
        let sync_cmd = format!(
            "claude-sync {}",
            git(&["rev-parse", "--show-toplevel"]).unwrap_or_default()
        );
        if has_rebuild {
            println!(
                "NOTE: claude-sync が要る（.claude が変わった）。{sync_cmd} か rebuild まで ~/.claude は古い版のまま"
            );
        } else {
            println!(
                "NOTE: claude-sync が要る（.claude が変わった）。{sync_cmd} まで ~/.claude は古い版のまま。取り込む側（nixfiles）の lock を進める前に rebuild すると、lock が固定した古い版に戻る"
            );
        }
    }
    Ok(())
}

/// 段階 3: 番号の予約のタグ。記録の形と番号の規則は design-records が持つ。既定のブランチ（リモートがあればリモートの）の
/// 履歴に無い記録のタグは消さない（消すと番号が再利用される）。解放できないときは、タグを残して後片付けを続ける（タグが
/// 残っても番号は再利用されず、次の prune で消える）。prune そのものが失敗したら止める。予約のタグは origin にある。
/// 2 の fetch は既定のブランチの追うリモートに行くので、fork のように upstream を追っていると origin に行かない
fn reservations() -> Result<(), i32> {
    let reachable = !ok(&["remote", "get-url", "origin"])
        || ok(&[
            "fetch",
            "-q",
            "--no-tags",
            "origin",
            "refs/tags/adr-*:refs/tags/adr-*",
            "refs/tags/issue-*:refs/tags/issue-*",
        ]);
    if git(&["tag", "--list", "adr-*", "issue-*"])
        .unwrap_or_default()
        .is_empty()
    {
        println!("RESERVATIONS_RELEASED: none");
        return Ok(());
    }
    if !design_records_available() {
        println!(
            "RESERVATIONS_RELEASED: skipped (design-records が PATH に無い。予約のタグは残した)"
        );
        return Ok(());
    }
    if !reachable {
        println!("RESERVATIONS_RELEASED: kept (origin に届かない。予約のタグは残した)");
        return Ok(());
    }
    match Command::new("design-records")
        .arg("prune")
        .stderr(std::process::Stdio::inherit())
        .output()
    {
        Err(_) => {
            println!(
                "RESERVATIONS_RELEASED: skipped (design-records が PATH に無い。予約のタグは残した)"
            );
            Ok(())
        }
        Ok(o) if o.status.success() => {
            let released: Vec<String> = String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect();
            println!(
                "RESERVATIONS_RELEASED: {}",
                if released.is_empty() {
                    "none".to_string()
                } else {
                    released.join(" ")
                }
            );
            Ok(())
        }
        Ok(_) => {
            eprintln!(
                "ERROR: 予約のタグを解放できません（design-records prune が失敗）。原因を直してから、もう一度実行してください"
            );
            Err(1)
        }
    }
}

fn design_records_available() -> bool {
    on_path("design-records")
}

/// 段階 5: リモートのブランチを消す
fn remote_branch(branch: &str, remote: &str, default: &str) -> Result<(), i32> {
    let on_remote = ok(&["remote", "get-url", remote])
        && git(&["ls-remote", "--heads", remote, branch]).is_some_and(|s| !s.is_empty());
    if !on_remote {
        println!("REMOTE_BRANCH: none");
        return Ok(());
    }
    if git(&["remote", "-v"])
        .unwrap_or_default()
        .contains("github.com")
    {
        if !gh_available() {
            println!("REMOTE_BRANCH: kept (gh がないため open PR の有無を確認できない)");
            return Ok(());
        }
        let open = Command::new("gh")
            .args([
                "pr", "list", "--head", branch, "--state", "open", "--json", "number", "--jq",
                "length",
            ])
            .output();
        let Some(n) = open.ok().filter(|o| o.status.success()).and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<u64>()
                .ok()
        }) else {
            println!("REMOTE_BRANCH: kept (open PR の有無を確認できない: gh pr list が失敗)");
            return Ok(());
        };
        if n > 0 {
            println!("REMOTE_BRANCH: kept (open PR {n} 件の head。削除すると PR が閉じる)");
            return Ok(());
        }
    }
    // リモートの既定のブランチがブランチの先端を含まなければ、消すとマージした内容がリモートのどの ref にも残らない
    // （ローカルでマージしただけで push していない）。push するかはユーザーが決めるので、残して知らせる
    if !ok(&[
        "merge-base",
        "--is-ancestor",
        &format!("{remote}/{branch}"),
        &format!("{remote}/{default}"),
    ]) {
        println!(
            "REMOTE_BRANCH: kept ({remote}/{default} がブランチの先端をまだ含まない。{default} を push してから消す)"
        );
        return Ok(());
    }
    step(&["push", "-q", remote, "--delete", branch])?;
    println!("REMOTE_BRANCH: deleted");
    Ok(())
}
