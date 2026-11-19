//! `post-merge-cleanup.sh <branch>`: PR のマージの後（または /git-merge の後）に、ブランチと worktree を片付ける。
//! 順は gh-pr-merge のスキルの「クリーンアップ」と、`claude/scripts/post-merge-cleanup.sh` の説明に書く:
//!
//! - （0 の前）cwd のリポジトリに branch の痕跡（ローカルのブランチ、リモート追跡の ref、リモートのブランチ、GitHub の
//!   リモートではそのブランチを head とするマージ済みの PR）が無ければ、何も変えずに止める。確かめられないときも止める
//! - 段階 0: ローカルの branch の先端が既定のブランチ（無ければ、追うリモートの既定のブランチを fetch したもの）の祖先で
//!   なければ、何も消さずに止める。第 2 引数（マージの前の既定のブランチ。/git-merge が渡す）を受けたら、その後の
//!   既定のブランチの第 1 親の列に、branch の先端を 2 番目以降の親に持つマージコミットが無ければ、何も消さずに止める
//! - 段階 1: branch をチェックアウトしている worktree を消す（既定のブランチの worktree に移ってから）。メインの worktree
//!   なら消さずに、段階 2 で既定のブランチへ切り替える（未コミットの変更があるか、既定のブランチが別の worktree にあれば、
//!   何も変えずに止まる）
//! - 段階 2: 既定のブランチへ切り替え、fetch --prune、追跡ブランチへ merge --ff-only（リモートが無ければ省く）。配置するものが変われば NOTE（差の起点は第 2 引数か、pull の前の HEAD）
//! - 段階 3: origin から予約のタグを取り、既定のブランチの履歴に記録が入った番号のタグを消す（design-records prune）
//! - 段階 4: ローカルのブランチを消す（-d）
//! - 段階 5: リモートのブランチを消す（リモートの既定のブランチが先端を含まないときと、open の PR の head か base のときは残す）
//!
//! git と gh の失敗のうち、続けられないものは、その標準エラーを出したまま、その終了コードで終える

use std::path::PathBuf;
use std::process::{Command, Output};

use crate::shell::sh_word;

const USAGE: &str = "Usage: post-merge-cleanup.sh <branch> [<マージの前の既定のブランチ>]";

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
        Ok(s) => Err(crate::child::exit_code(s)),
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

/// PATH で name を探したパス（Windows は PATHEXT の .cmd なども探す。起動はしない。--version などに答えない偽物もある）
fn tool(name: &str) -> Option<PathBuf> {
    crate::staged_checks::find_on_path(name)
}

fn gh_available() -> bool {
    tool("gh").is_some()
}

/// 見つけたパスの gh（Command は拡張子の無い名前に .exe しか足さない）
fn gh() -> Command {
    Command::new(tool("gh").unwrap_or_else(|| PathBuf::from("gh")))
}

pub fn main(args: &[String]) -> i32 {
    let (branch, base) = match args {
        [branch] => (branch, None),
        [branch, base] if !base.is_empty() => (branch, Some(base.as_str())),
        _ => {
            eprintln!("{USAGE}");
            return 1;
        }
    };
    if branch.is_empty() {
        eprintln!("{USAGE}");
        return 1;
    }
    match cleanup(branch, base) {
        Ok(()) => 0,
        Err(code) => code,
    }
}

fn cleanup(branch: &str, base: Option<&str>) -> Result<(), i32> {
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
    // 何も変える前に、渡されたマージの前の既定のブランチを確かめる（NOTE の差の起点になる）
    let before = match base {
        None => None,
        Some(rev) => {
            let Some(commit) = git(&["rev-parse", "--verify", "-q", &format!("{rev}^{{commit}}")])
            else {
                eprintln!(
                    "ERROR: マージの前の既定のブランチ {rev} がコミットとして解決できません。何も変えていません"
                );
                return Err(1);
            };
            if !ok(&[
                "merge-base",
                "--is-ancestor",
                &commit,
                &format!("refs/heads/{default}"),
            ]) {
                eprintln!(
                    "ERROR: {rev} は {default} の祖先ではありません（マージの前の {default} を渡す）。何も変えていません"
                );
                return Err(1);
            }
            Some(commit)
        }
    };
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
    // メインの worktree でチェックアウトしているブランチ（worktree を作らずに作業した）は、メインの worktree を消さずに、
    // 段階 2 で既定のブランチへ切り替える
    let branch_wt = worktree_of(&list, branch);
    let in_main = branch_wt.as_deref() == Some(main_root.as_str());
    let wt_path = branch_wt.filter(|_| !in_main);
    let default_root = worktree_of(&list, default);
    // 以降は既定のブランチの worktree（どこにもチェックアウトされていなければメインの worktree）で行う。今いる
    // worktree が対象のものでも、無関係なブランチのものでも、そこでは既定のブランチへ switch できない
    let base = PathBuf::from(default_root.clone().unwrap_or_else(|| main_root.clone()));
    if let Err(e) = std::env::set_current_dir(&base) {
        eprintln!("ERROR: {} に移れません: {e}", base.display());
        return Err(1);
    }

    // 手元だけで決まる確認を、ネットワークに行く痕跡とマージ済みかの確認（fetch する）より先にする
    if in_main {
        main_worktree_can_switch(branch, default, &main_root, default_root.as_deref())?;
    }
    trace(branch, &remote, &main_root)?;
    merged(branch, default, default_root.as_deref())?;
    if let Some(before) = &before {
        merge_commit_of(branch, default, before)?;
    }

    match &wt_path {
        Some(wt) => {
            step(&["worktree", "remove", wt])?;
            println!("WORKTREE_REMOVED: {wt}");
        }
        None => println!("WORKTREE_REMOVED: none"),
    }

    update_default(default, before.as_deref())?;
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

/// メインの worktree でチェックアウトしているブランチは、段階 2 でメインの worktree を既定のブランチへ切り替えて外す。
/// 未コミットの変更があると既定のブランチへ持ち込むので、linked worktree の remove と同じく止める。既定のブランチが別の
/// worktree にあると切り替えられず、後の branch -d で止まるので、先に止めて外し方を示す（どちらも何も変えない）
fn main_worktree_can_switch(
    branch: &str,
    default: &str,
    main_root: &str,
    default_root: Option<&str>,
) -> Result<(), i32> {
    if git(&["-C", main_root, "status", "--porcelain"]).is_none_or(|s| !s.is_empty()) {
        eprintln!(
            "ERROR: {branch} をチェックアウトしているメインの worktree（{main_root}）に未コミットの変更があります。何も変えていません"
        );
        eprintln!(
            "  修正方法: 変更を退避する（git stash、または {default} から作った新しいブランチにコミットする）か捨ててから、もう一度実行する。マージ済みの {branch} にコミットすると、マージしていないブランチになって次の実行が止まる"
        );
        return Err(1);
    }
    if let Some(other) = default_root {
        eprintln!(
            "ERROR: {branch} はメインの worktree（{main_root}）でチェックアウトされ、{default} は別の worktree（{other}）にあるので、メインの worktree を {branch} から外せません。何も変えていません"
        );
        eprintln!(
            "  修正方法: メインの worktree で別のブランチに切り替える（git -C {main_root} switch --detach など）か、{other} を片付けてから、もう一度実行する"
        );
        return Err(1);
    }
    Ok(())
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
    let out = gh()
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

/// 段階 0 の続き: before（マージの前の既定のブランチ）から既定のブランチまでの第 1 親の列に、branch の先端を 2 番目以降の
/// 親に持つマージコミットがあるか。先端が fast-forward やほかのブランチのマージで既定のブランチに入り、/git-merge の
/// `git merge --no-ff` がこのブランチのマージコミットを作っていないことを、ブランチを消す前に見つける。マージの後の
/// コミットは通す（最後のコミットの第 2 親だけを見ると、続けて積んだコミットで止まる）。ローカルのブランチが無ければ
/// （消した後のやり直し）見ない
fn merge_commit_of(branch: &str, default: &str, before: &str) -> Result<(), i32> {
    let local = format!("refs/heads/{branch}");
    // show-ref の終了 1 は「無い」。ほかの失敗を無いと読むと、確かめずにブランチを消す
    let lookup = run(&["show-ref", "--verify", "--quiet", &local]);
    match lookup.as_ref().and_then(|o| o.status.code()) {
        Some(0) => {}
        Some(1) => return Ok(()),
        _ => {
            if let Some(o) = &lookup {
                eprint!("{}", String::from_utf8_lossy(&o.stderr));
            }
            eprintln!(
                "ERROR: ローカルのブランチ {branch} の有無を確かめられません（git show-ref が失敗）。何も消していません"
            );
            return Err(1);
        }
    }
    let (Some(tip), Some(merges)) = (
        git(&[
            "rev-parse",
            "--verify",
            "-q",
            &format!("{local}^{{commit}}"),
        ]),
        git(&[
            "rev-list",
            "--first-parent",
            "--merges",
            "--parents",
            &format!("{before}..refs/heads/{default}"),
        ]),
    ) else {
        eprintln!(
            "ERROR: {branch} のマージコミットを確かめられません（git rev-parse か git rev-list が失敗）。何も消していません"
        );
        return Err(1);
    };
    // 行は「マージコミット 第 1 親 第 2 親 …」
    if merges
        .lines()
        .any(|l| l.split_whitespace().skip(2).any(|p| p == tip))
    {
        return Ok(());
    }
    eprintln!(
        "ERROR: {branch} の先端 {tip} は {default} に入っているが、{before} の後の {default} に、それを第 2 親に持つマージコミットがありません（fast-forward で入った、ほかのブランチのマージで入った、または第 2 引数がマージの前の {default} でない）。何も消していません"
    );
    eprintln!(
        "  修正方法: 先に、第 2 引数がマージの第 1 親（マージの前の {default}）かを確かめる。合っていれば、git log --first-parent --oneline {before}..{default} で入り方を確かめる。fast-forward で、その後に積んだコミットが {default} に無ければ、{default} の worktree で git reset --keep {before} で戻してから git merge --no-ff {branch} をやり直し、そのマージの第 1 親を第 2 引数に渡して、もう一度実行する（戻さずにマージをやり直すと、Already up to date で何もしない）。後に積んだコミットがあるか、ほかのブランチのマージで入ったなら、戻さずにユーザーに確認する"
    );
    Err(1)
}

/// 段階 2: 既定のブランチを最新にする。hook のバイナリと ~/.claude は rebuild か claude-sync で配置される。pull しただけでは
/// 古いまま動くので知らせる（自動では実行しない。rebuild は sudo が要り、走っている別のセッションの hook も入れ替わる）。
/// 判定は広く取る（配置するものを 1 つずつ挙げると漏れ、多めに知らせても害は無い）
///
/// 差の起点は、呼ぶ側が渡したマージの前の既定のブランチ（/git-merge は pull より前に手元で main を進めるので、pull の前後だけを
/// 見ると差が空になる）。渡されなければ pull の前の HEAD（reflog から取らないのは、reflog はマージのほかの更新も含み、
/// 期限で消え、どのエントリが今回のマージの前かを決められないため。#4）
fn update_default(default: &str, base: Option<&str>) -> Result<(), i32> {
    if git(&["branch", "--show-current"]).as_deref() != Some(default) {
        step(&["switch", "-q", default])?;
    }
    let mut before = base
        .map(str::to_string)
        .or_else(|| git(&["rev-parse", "HEAD"]))
        .unwrap_or_default();
    // リモートの無いリポジトリ（/git-merge でローカルにマージした後）では最新にする先が無い
    if !git(&["remote"]).unwrap_or_default().is_empty() {
        step(&["fetch", "-q", "--prune"])?;
        if base.is_none() {
            before = git(&["rev-parse", "HEAD"]).unwrap_or_default();
        }
        // pull は FETCH_HEAD を読み、同じ clone の別の fetch が書き換えると Cannot rebase onto multiple branches で止まる。
        // fetch の後の追跡ブランチへ進める merge は FETCH_HEAD に頼らない
        step(&["merge", "-q", "--ff-only", "@{upstream}"])?;
    }
    // ~/.claude の配置の元（ハーネス）は同期のコマンド（claude-setup の crate。古い版の clone と、ハーネスを分ける前の
    // nixfiles は claude-sync.sh）を持ち、rebuild する構成のリポジトリ（nixfiles）は nix-rebuild.sh を持つ
    // 目印は差の起点と HEAD のどちらかにあればよい（目印を消すコミットを取り込んでも、その変更に知らせが要る）。
    // ls-tree は、無いときは空の出力で成功し、オブジェクトの破損などでは失敗する。後者は無いことにせず止める
    let has = |path: &str| -> Result<bool, i32> {
        for rev in [before.as_str(), "HEAD"] {
            match git(&["ls-tree", "--name-only", rev, "--", path]) {
                Some(s) if !s.is_empty() => return Ok(true),
                Some(_) => {}
                None => {
                    eprintln!(
                        "ERROR: {rev} の {path} の有無を確かめられません（git ls-tree が失敗）"
                    );
                    return Err(1);
                }
            }
        }
        Ok(false)
    };
    let harness = has("crates/claude-setup/Cargo.toml")?;
    let has_sync = harness || has("modules/home/scripts/claude-sync.sh")?;
    let has_rebuild = has("modules/home/scripts/nix-rebuild.sh")?;
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
    } else if has_sync
        && changed(&[
            "crates",
            "nix",
            "claude-settings.json",
            "flake.nix",
            "flake.lock",
        ])
    {
        // ハーネスの hook とモジュールは、取り込む側（nixfiles）の lock が固定した版で配置されるので、lock を進めてから rebuild する
        println!(
            "NOTE: lock の更新と rebuild が要る（crates、nix、claude-settings.json か flake が変わった）。取り込む側（nixfiles）で nix-rebuild update を実行するまで hook や設定は古い版のまま動く"
        );
    }
    // 配る元（同期のコマンドが置くもの）。claude-setup の crate を crates/ に持つ版のハーネスは claude、config、共通の
    // 辞書で、.claude はこのリポジトリだけの指示とスキル（その場で読まれ、配らない。この版はどれも claude/ を持つ）。
    // claude-sync.sh を持つ版（古い版のハーネスと、ハーネスを分ける前の nixfiles）は .claude が配る元。同期のコマンドを
    // 持たないリポジトリの .claude はプロジェクトの指示で、何も知らせない
    let sources: &[&str] = if harness {
        &["claude", "config", "crates/ja-terms/prh.yml"]
    } else {
        &["claude", "config", ".claude"]
    };
    if has_sync && changed(sources) {
        // 引数なしの claude-sync は既定の clone を読む。pull したのがそこと限らないので、このリポジトリを元に渡す形で示す
        let sync_cmd = format!(
            "claude-sync {}",
            git(&["rev-parse", "--show-toplevel"]).unwrap_or_default()
        );
        if has_rebuild {
            println!(
                "NOTE: claude-sync が要る（配る元が変わった）。{sync_cmd} か rebuild まで ~/.claude は古い版のまま"
            );
        } else {
            println!(
                "NOTE: claude-sync が要る（配る元が変わった）。{sync_cmd} まで ~/.claude は古い版のまま。取り込む側（nixfiles）の lock を進める前に rebuild すると、lock が固定した古い版に戻る"
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
    match Command::new(tool("design-records").unwrap_or_else(|| PathBuf::from("design-records")))
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
    tool("design-records").is_some()
}

/// 段階 5: リモートのブランチを消す
fn remote_branch(branch: &str, remote: &str, default: &str) -> Result<(), i32> {
    let on_remote = ok(&["remote", "get-url", remote])
        && git(&[
            "ls-remote",
            "--heads",
            remote,
            &format!("refs/heads/{branch}"),
        ])
        .is_some_and(|s| !s.is_empty());
    if !on_remote {
        println!("REMOTE_BRANCH: none");
        return Ok(());
    }
    let github = git(&["remote", "-v"])
        .unwrap_or_default()
        .contains("github.com");
    if github {
        if !gh_available() {
            println!("REMOTE_BRANCH: kept (gh がないため open PR の有無を確認できない)");
            return Ok(());
        }
        let open = gh()
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
    // （ローカルでマージしただけで push していない）。push するかはユーザーが決めるので、残して知らせる。上の PR が
    // あれば、push の後に消すだけでも閉じるので、ここでも示す
    let stacked = stacked_prs(branch, remote);
    if !ok(&[
        "merge-base",
        "--is-ancestor",
        &format!("{remote}/{branch}"),
        &format!("{remote}/{default}"),
    ]) {
        let unpushed = format!("{remote}/{default} がブランチの先端をまだ含まない");
        match stacked {
            // push していない既定のブランチへ付け替えると、上の PR の差分にこのブランチのコミットが入るので、push を先に書く
            Stacked::Found(found) => println!(
                "REMOTE_BRANCH: kept ({unpushed}。{}の base でもある。{default} を push し、次のコマンドで上の PR の base を付け替えてから（要らない PR なら閉じてから）、ブランチを消す)\n{}",
                listed(&found),
                commands(&found, branch, remote, default)
            ),
            Stacked::Unknown(why) => println!(
                "REMOTE_BRANCH: kept ({unpushed}。{default} を push してから消す。{branch} を base にした open PR の有無も確認できない: {why})"
            ),
            Stacked::None => {
                println!("REMOTE_BRANCH: kept ({unpushed}。{default} を push してから消す)")
            }
        }
        return Ok(());
    }
    match stacked {
        Stacked::Found(found) => {
            println!(
                "REMOTE_BRANCH: kept ({}の base。削除すると GitHub がそれらの PR を閉じ、閉じた PR は base を付け替えられない。\
                 次のコマンドで上の PR の base を付け替えてから（要らない PR なら閉じてから）、ブランチを消す)\n{}",
                listed(&found),
                commands(&found, branch, remote, default)
            );
            Ok(())
        }
        Stacked::Unknown(why) => {
            println!(
                "REMOTE_BRANCH: kept ({branch} を base にした open PR の有無を確認できない: {why})"
            );
            Ok(())
        }
        Stacked::None => {
            step(&["push", "-q", remote, "--delete", branch])?;
            println!("REMOTE_BRANCH: deleted");
            Ok(())
        }
    }
}

/// remote の branch を base にした開いている PR（積み重ねた上の PR）。API でブランチを消すと GitHub はそれを base にした
/// 開いている PR を閉じ、閉じた PR は base を付け替えられない
enum Stacked {
    /// 無い（push 先に GitHub のリポジトリが無ければ、消しても GitHub の PR は閉じない）
    None,
    /// 確かめられない理由
    Unknown(String),
    /// push 先の GitHub のリポジトリ（owner/name）ごとの、上の PR の番号（空のものは入れない）
    Found(Vec<(String, Vec<String>)>),
}

/// 探し方は pre-merge-check の検査 10（`stacked_prs_reason`）と揃える。上の PR は、消すブランチを持つリポジトリにある
/// ので、gh の既定のリポジトリ（fork の clone では親）ではなく、git push --delete が消しに行く URL（pushurl があれば
/// その全部、無ければ url の全部）のリポジトリを探す。URL は、insteadOf と pushInsteadOf で書き換えた先
/// （git remote get-url --push --all）と、書き換える前の設定の値の両方を読む。書き換えた先だけでは略記（gh: など）
/// でない GitHub の URL をミラーへ書き換えた設定を、書き換える前だけでは略記を読めない。両方を読むと、書き換えた先が
/// ミラーのときに残しすぎるが、上の PR を閉じる側には倒れない
fn stacked_prs(branch: &str, remote: &str) -> Stacked {
    let configured = [
        format!("remote.{remote}.pushurl"),
        format!("remote.{remote}.url"),
    ]
    .iter()
    .filter_map(|key| git(&["config", "--get-all", key]))
    .find(|v| !v.is_empty())
    .unwrap_or_default();
    let rewritten = git(&["remote", "get-url", "--push", "--all", remote]).unwrap_or_default();
    let mut repos: Vec<String> = Vec::new();
    for url in configured.lines().chain(rewritten.lines()) {
        match github_repo(url) {
            // GitHub の owner と name は大文字と小文字を区別しない
            Some(repo) if !repos.iter().any(|r| r.eq_ignore_ascii_case(&repo)) => repos.push(repo),
            Some(_) => {}
            None if on_github(url) => {
                return Stacked::Unknown(format!(
                    "{remote} の URL（{url}）から GitHub のリポジトリを読めない"
                ));
            }
            None => {}
        }
    }
    if repos.is_empty() {
        return Stacked::None;
    }
    if !gh_available() {
        return Stacked::Unknown("gh がない".to_string());
    }
    let mut found = Vec::new();
    for repo in repos {
        let out = gh()
            .args([
                "pr",
                "list",
                "-R",
                &repo,
                "--base",
                branch,
                "--state",
                "open",
                "--json",
                "number",
                "--jq",
                ".[].number",
            ])
            .output();
        let Some(out) = out.ok().filter(|o| o.status.success()) else {
            return Stacked::Unknown("gh pr list が失敗".to_string());
        };
        let numbers: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect();
        if !numbers.is_empty() {
            found.push((repo, numbers));
        }
    }
    if found.is_empty() {
        Stacked::None
    } else {
        Stacked::Found(found)
    }
}

/// 上の PR の件数と番号（`open PR 2 件（#12, #13）`。リポジトリが複数なら `owner/name#12`）
fn listed(found: &[(String, Vec<String>)]) -> String {
    let numbers: Vec<String> = found
        .iter()
        .flat_map(|(repo, numbers)| {
            let prefix = if found.len() > 1 { repo.as_str() } else { "" };
            numbers.iter().map(move |n| format!("{prefix}#{n}"))
        })
        .collect();
    format!("open PR {} 件（{}）", numbers.len(), numbers.join(", "))
}

/// 上の PR ごとの base の付け替えと、その後にブランチを消すコマンドの行（行頭に空白 2 つ）。付け替えは検査 10 と同じ形
fn commands(found: &[(String, Vec<String>)], branch: &str, remote: &str, default: &str) -> String {
    let mut lines: Vec<String> = found
        .iter()
        .flat_map(|(repo, numbers)| {
            numbers.iter().map(move |n| {
                format!(
                    "  gh pr edit {n} --base {} -R {}",
                    sh_word(default),
                    sh_word(repo)
                )
            })
        })
        .collect();
    // このスクリプトのやり直しでなく、削除のコマンドを示す。ローカルのブランチを消した後は branch.<name>.remote が
    // 読めず、やり直しは origin 以外のリモートを探せない
    lines.push(format!(
        "  git push {} --delete {}",
        sh_word(remote),
        sh_word(branch)
    ));
    lines.join("\n")
}

/// URL のホストとパス（`<scheme>://[<user>@]<host>[:<port>]/<path>` の形と、git の scp の形 `[<user>@]<host>:<path>`）
fn host_and_path(url: &str) -> Option<(&str, &str)> {
    match url.split_once("://") {
        Some((_, rest)) => {
            let (authority, path) = rest.split_once('/')?;
            let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
            Some((host.split_once(':').map_or(host, |(h, _)| h), path))
        }
        None => {
            // git は、最初の / より前に : があるものを scp の形と読む（無ければローカルのパス）
            let (authority, path) = url.split_once(':')?;
            if authority.contains('/') {
                return None;
            }
            Some((
                authority.rsplit_once('@').map_or(authority, |(_, h)| h),
                path,
            ))
        }
    }
}

/// リモートの URL のホストが GitHub か（大文字と小文字は区別しない）
fn on_github(url: &str) -> bool {
    host_and_path(url).is_some_and(|(host, _)| host.eq_ignore_ascii_case("github.com"))
}

/// リモートの URL が GitHub のリポジトリを指すなら、その owner/name
fn github_repo(url: &str) -> Option<String> {
    if !on_github(url) {
        return None;
    }
    let (_, path) = host_and_path(url)?;
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    match path.split('/').collect::<Vec<_>>().as_slice() {
        [owner, name] if !owner.is_empty() && !name.is_empty() => Some(format!("{owner}/{name}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{github_repo, on_github};

    #[test]
    fn on_github_matches_the_host_not_a_substring() {
        assert!(on_github("https://github.com/octo"));
        assert!(on_github("git@GitHub.com:octo"));
        assert!(!on_github(
            "https://github.com.mirror.example/octo/repo.git"
        ));
        assert!(!on_github("/srv/github.com/repo.git"));
    }

    #[test]
    fn github_repo_reads_https_ssh_and_scp_like_urls() {
        for url in [
            "https://github.com/octo/repo.git",
            "https://github.com/octo/repo",
            "https://github.com/octo/repo/",
            "https://user@github.com/octo/repo.git",
            "ssh://git@github.com/octo/repo.git",
            "git@github.com:octo/repo.git",
            "git@github.com:octo/repo",
            "org-123@github.com:octo/repo.git",
            "github.com:octo/repo.git",
            "ssh://git@github.com:22/octo/repo",
            "https://GitHub.com/octo/repo",
        ] {
            assert_eq!(github_repo(url).as_deref(), Some("octo/repo"), "{url}");
        }
    }

    #[test]
    fn github_repo_is_none_for_other_hosts_paths_and_partial_urls() {
        for url in [
            "/srv/git/repo.git",
            "https://gitlab.com/octo/repo.git",
            "https://github.com.evil.example/octo/repo.git",
            "https://github.com/octo",
            "https://github.com/octo/repo/extra",
            "git@github.com:repo.git",
        ] {
            assert_eq!(github_repo(url), None, "{url}");
        }
    }
}
