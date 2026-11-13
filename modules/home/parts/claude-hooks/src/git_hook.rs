//! `claude-hooks git-hook <名前> [引数]...`: グローバルの git hook の中身（ADR-0086）。hook のファイルは、これを呼ぶだけの
//! sh で、modules/home/parts/git-hooks.nix が置く（Windows の設置は、4 つの hook を移した後。ADR-0086）。終了コードは git に返す
//! （0 以外は git がコミットを止める）。
//!
//! - commit-msg <メッセージのファイル>: ローカルの commit-msg があればそれに任せる。Claude Code のセッション
//!   （`CLAUDECODE=1`）のコミットで、fork でなければ、`ja-terms commit-msg` に渡す（Conventional Commits、件名の長さ、
//!   崩れた日本語の語。PreToolUse hook の check-commit-message が同じ検査を先に呼ぶ）。手で打つコミットの自由を残す
//!   ため、Claude Code の外のコミットは検査しない
//! - post-rewrite <amend|rebase>: rebase と amend で変わったコミットの番号を、作業ツリーの追跡している文書の中で
//!   付け直す（commit-refs remap。ADR-0012）。測ったコミットの番号を文書に書くリポジトリだけが要るので、
//!   `git config remap.commitRefs true` でオプトインする。標準入力の旧と新の対応は一度しか渡されないので、取っておいて
//!   ローカルの post-rewrite（あれば先に呼ぶ）と commit-refs の両方に渡す。置き換えはコミットしない（どのコミットに
//!   含めるかは書き換えた人が決める）。git は書き換えの後に呼び、この hook の失敗は書き換えを止めない（表示するだけ）

use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::git;
use crate::output;
use crate::rules::ja_terms;

const USAGE: &str = "Usage: claude-hooks git-hook commit-msg <メッセージのファイル>
       claude-hooks git-hook post-rewrite <amend|rebase>";

/// commit-refs の store のパスを持つ環境変数。package.nix のラッパーが固定する（crate::tools::find）
const COMMIT_REFS: &str = "CLAUDE_HOOKS_COMMIT_REFS";

pub fn main(args: &[String]) -> i32 {
    let code = match args {
        [name, rest @ ..] if name == "commit-msg" => commit_msg(rest),
        [name, rest @ ..] if name == "post-rewrite" => post_rewrite(rest),
        _ => {
            eprintln!("{USAGE}");
            2
        }
    };
    // git を起動できなかった理由（ローカルの hook と fork を判定できていない）を見せる
    for notice in output::take_notices() {
        eprintln!("{notice}");
    }
    code
}

fn commit_msg(args: &[String]) -> i32 {
    let dir = Path::new(".");
    // ローカルの hook を探せないまま通すと、プロジェクトの commit-msg（Change-Id を足すものなど）を飛ばすので止める
    let Some(common) = git::git(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ) else {
        eprintln!(
            "❌ git rev-parse --git-common-dir に失敗したので、ローカルの commit-msg を探せません（同じディレクトリで git rev-parse --path-format=absolute --git-common-dir を実行すると、git の理由が出ます）"
        );
        return 1;
    };
    // ローカルの hook には、git から受けた引数をそのまま渡す（引数の数を確かめるのはこちらの検査だけ）
    if let Some(local) = git::local_hook(Path::new(&common), "commit-msg") {
        return run_local(&local, args);
    }
    let [file] = args else {
        eprintln!("{USAGE}");
        return 2;
    };
    if std::env::var("CLAUDECODE").as_deref() != Ok("1") {
        return 0;
    }
    // 他人のプロジェクトの fork では、Conventional Commits も件名の長さも日本語の語の辞書もこちらの規約で、上流の
    // 慣習（"Fix socket transport when …" のような文）と衝突して上流向けのコミットを歪める
    if git::is_fork(dir) {
        println!(
            "⏭️  upstream リモートのある fork なので、こちらの規約の検査（Conventional Commits、件名の長さ、日本語の語）を飛ばします"
        );
        return 0;
    }
    let Some(ja_terms) = ja_terms() else {
        eprintln!(
            "❌ ja-terms が見つからないので、コミットのメッセージを検査できません（Linux は claude-hooks の package.nix が CLAUDE_HOOKS_JA_TERMS を渡す）"
        );
        return 1;
    };
    status(
        Command::new(&ja_terms).arg("commit-msg").arg(file),
        &ja_terms,
    )
}

/// ローカルの hook を、git から受けた引数のまま実行する。sh に exec させる: sh の exec は shebang の行を読んでその
/// インタプリタで起動し、shebang の無い実行ファイルは sh のスクリプトとして実行する（git が hook を起動するのと同じ）。
/// Windows はスクリプトを直に起動できないので、どちらにしても sh が要る。Unix はグローバルの hook のファイルの
/// shebang と同じ /bin/sh、Windows は PATH の Git for Windows の sh
fn run_local(hook: &Path, args: &[String]) -> i32 {
    status(&mut local_command(hook, args), &hook.display().to_string())
}

fn local_command(hook: &Path, args: &[String]) -> Command {
    let sh = if cfg!(windows) { "sh" } else { "/bin/sh" };
    let mut cmd = Command::new(sh);
    cmd.arg("-c").arg("exec \"$0\" \"$@\"").arg(hook).args(args);
    cmd
}

/// cmd に input を標準入力で渡して実行し、終了コードと標準出力を返す（標準エラーは git に渡す）。起動できなければ
/// 理由を出して None
fn run_with_input(
    cmd: &mut Command,
    input: &[u8],
    capture: bool,
    what: &str,
) -> Option<(i32, Vec<u8>)> {
    cmd.stdin(Stdio::piped());
    if capture {
        cmd.stdout(Stdio::piped());
    }
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            eprintln!("❌ {what} を起動できません（{e}）");
            return None;
        }
    };
    // 入力を読まずに終わる hook もあるので、書けないこと（EPIPE）は失敗にしない
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input);
    }
    match child.wait_with_output() {
        Ok(out) => Some((out.status.code().unwrap_or(1), out.stdout)),
        Err(e) => {
            eprintln!("❌ {what} の終わりを待てません（{e}）");
            None
        }
    }
}

fn post_rewrite(args: &[String]) -> i32 {
    let mut input = Vec::new();
    if let Err(e) = std::io::stdin().read_to_end(&mut input) {
        eprintln!("❌ git が渡した旧と新のコミットの対応を読めません（{e}）");
        return 1;
    }
    let dir = Path::new(".");
    let Some(common) = git::git(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ) else {
        eprintln!(
            "❌ git rev-parse --git-common-dir に失敗したので、ローカルの post-rewrite を探せません（同じディレクトリで git rev-parse --path-format=absolute --git-common-dir を実行すると、git の理由が出ます）"
        );
        return 1;
    };
    // ローカルの hook の失敗でも付け直しは行い、その終了コードを最後に返す（ローカルの hook があるだけで付け直しが
    // 外れないように）
    let local_rc = match git::local_hook(Path::new(&common), "post-rewrite") {
        Some(hook) => {
            let what = hook.display().to_string();
            run_with_input(&mut local_command(&hook, args), &input, false, &what)
                .map_or(1, |(code, _)| code)
        }
        None => 0,
    };
    if git::git(dir, &["config", "--type=bool", "--get", "remap.commitRefs"]).as_deref()
        != Some("true")
    {
        return local_rc;
    }
    // rebase の途中（edit で止めた所）の amend でも git は呼ぶが、rebase の終わりにもう一度、元のコミットから最後の
    // コミットへの対応を渡す。途中で置き換えると作業ツリーを汚して rebase --continue に混ざり、途中の番号に置き換えた
    // 文書は最後の対応で引けなくなる
    if args.first().map(String::as_str) == Some("amend") && rebasing(dir) {
        return local_rc;
    }
    let Some(git_dir) = git::git(dir, &["rev-parse", "--absolute-git-dir"]) else {
        eprintln!(
            "❌ git rev-parse --absolute-git-dir に失敗したので、コミットの番号を付け直せません"
        );
        return 1;
    };
    // 失敗したときに対応を残す場所。書き換えは済んでいて、この対応は二度と渡されないので、直してから手で呼び直せるようにする
    let saved = Path::new(&git_dir).join("commit-refs-remap.input");
    println!("🔁 文書の中のコミットの番号を付け直します（remap.commitRefs）");
    let out = match crate::tools::find(COMMIT_REFS, "commit-refs") {
        Some(bin) => run_with_input(Command::new(&bin).arg("remap"), &input, true, &bin),
        None => {
            eprintln!(
                "❌ commit-refs が見つかりません（Linux は claude-hooks の package.nix が {COMMIT_REFS} を渡す）"
            );
            None
        }
    };
    let stdout = match out {
        Some((0, stdout)) => stdout,
        _ => {
            if let Err(e) = std::fs::write(&saved, &input) {
                eprintln!("❌ 対応を {} に残せません（{e}）", saved.display());
            }
            println!(
                "❌ コミットの番号を付け直せませんでした（書き換えたかどうかは、上の ERROR の最後の行にあります）"
            );
            // 貼って使えるよう、パスをシェルの引数の形にする（空白などを含むリポジトリのため）
            println!(
                "   修正方法: 上の ERROR を直してから、commit-refs remap {}",
                sh_quote(&saved.display().to_string())
            );
            return 1;
        }
    };
    let text = String::from_utf8_lossy(&stdout);
    print!("{text}");
    if !text.ends_with('\n') && !text.is_empty() {
        println!();
    }
    if !text.lines().any(|line| line == "REFS: 0") {
        println!("📝 置き換えはコミットしていません。git diff で確かめて、コミットしてください");
    }
    local_rc
}

/// rebase の途中か（rebase-merge か rebase-apply のディレクトリがある）
fn rebasing(dir: &Path) -> bool {
    ["rebase-merge", "rebase-apply"].iter().any(|name| {
        git::git(dir, &["rev-parse", "--git-path", name]).is_some_and(|p| dir.join(p).is_dir())
    })
}

/// sh の 1 つの引数として貼れる形（安全な字だけなら、そのまま）
fn sh_quote(s: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "/._-:@%+=,".contains(c);
    if !s.is_empty() && s.chars().all(safe) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// 起動して、終了コードを返す（起動できなければ止める）
fn status(cmd: &mut Command, what: &str) -> i32 {
    match cmd.status() {
        // シグナルで終わったら（code が無い）止める
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("❌ {what} を起動できません（{e}）");
            1
        }
    }
}
