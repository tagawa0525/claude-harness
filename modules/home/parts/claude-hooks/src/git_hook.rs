//! `claude-hooks git-hook <名前> [引数]...`: グローバルの git hook の中身（ADR-0086）。hook のファイルは、これを呼ぶだけの
//! sh で、modules/home/parts/git-hooks.nix が置く（Windows の設置は、4 つの hook を移した後。ADR-0086）。終了コードは git に返す
//! （0 以外は git がコミットを止める）。
//!
//! - commit-msg <メッセージのファイル>: ローカルの commit-msg があればそれに任せる。Claude Code のセッション
//!   （`CLAUDECODE=1`）のコミットで、fork でなければ、`ja-terms commit-msg` に渡す（Conventional Commits、件名の長さ、
//!   崩れた日本語の語。PreToolUse hook の check-commit-message が同じ検査を先に呼ぶ）。手で打つコミットの自由を残す
//!   ため、Claude Code の外のコミットは検査しない
//! - pre-commit: Claude Code のセッションの main / master への直接のコミットを止め（crate::rules::main_commit_blocked。
//!   PreToolUse hook と同じ関数。git の hook は必ず対象のリポジトリの中で走るので、worktree や cd の失敗でも誤判定
//!   しない）、マージを締めるコミット（MERGE_HEAD がある。git は pre-merge-commit を呼ばない）では隣の pre-merge-commit を
//!   呼ぶ。ローカルの pre-commit があれば、それと機密情報の検査だけを行う（ローカルの hook が index を変えた後に検査
//!   する）。無ければ、プロジェクトの .pre-commit-config.yaml を prek で（設定はそのプロジェクトの規約なので fork でも）、
//!   ステージしたファイルがあれば、fork でなければ既定の検査（crate::staged_checks）と崩れた日本語の語（`ja-terms
//!   staged`）を、最後に機密情報の検査を行う
//! - pre-merge-commit: main / master へのマージコミットを、マージ結果が品質チェック（~/.claude の run-checks.sh
//!   --merge）を通ったときだけ作る。git はマージを用意した後（作業ツリーと index がマージ結果の状態）、コミットの前に
//!   呼ぶ。ローカルの pre-merge-commit を先に呼び（置き換えない。置き換えると、ローカルの hook があるだけでゲートが外れる）、
//!   機密情報の検査はどのブランチへのマージでも行う（競合の無いマージでは pre-commit が走らない）。ほかのブランチへの
//!   マージは途中経過なので品質チェックをしない。fork でも飛ばす。作業ツリーにマージ結果以外の変更があると、別の内容を
//!   検査してしまうので止める
//! - scan-secrets: 機密情報の検査（ADR-0015）。Claude Code のセッション（`CLAUDECODE=1`）のコミットで、コミットに
//!   入る中身（index と HEAD の差）を gitleaks で見る（crate::gitleaks）。グローバルの pre-commit と pre-merge-commit
//!   が、index を変える検査の後に呼ぶ。漏えいと、検査できないこと（gitleaks の失敗）を分けて出し、どちらも 1 で
//!   止める。迂回は ALLOW_SECRET_COMMIT=<理由>（PreToolUse hook と同じ規則。ADR-0023）
//! - staged-checks [言語]...: 既定の検査（ステージした版の書式と lint。crate::staged_checks）
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
use crate::process::Unfinished;
use crate::rules::ja_terms;

const USAGE: &str = "Usage: claude-hooks git-hook pre-commit
       claude-hooks git-hook pre-merge-commit
       claude-hooks git-hook commit-msg <メッセージのファイル>
       claude-hooks git-hook scan-secrets
       claude-hooks git-hook staged-checks [nix|python|markdown|rust]...
       claude-hooks git-hook post-rewrite <amend|rebase>";

/// gitleaks の store のパスを持つ環境変数。package.nix のラッパーが固定する（crate::tools::find）
const GITLEAKS: &str = "CLAUDE_HOOKS_GITLEAKS";

/// commit-refs の store のパスを持つ環境変数。package.nix のラッパーが固定する（crate::tools::find）
const COMMIT_REFS: &str = "CLAUDE_HOOKS_COMMIT_REFS";

pub fn main(args: &[String]) -> i32 {
    let code = match args {
        [name, rest @ ..] if name == "pre-commit" => pre_commit(rest),
        [name, rest @ ..] if name == "pre-merge-commit" => pre_merge_commit(rest),
        [name, rest @ ..] if name == "commit-msg" => commit_msg(rest),
        [name, rest @ ..] if name == "post-rewrite" => post_rewrite(rest),
        [name] if name == "scan-secrets" => scan_secrets(),
        [name, langs @ ..] if name == "staged-checks" => crate::staged_checks::run(langs),
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
    let Ok(local) = find_local_hook(dir, "commit-msg") else {
        return 1;
    };
    // ローカルの hook には、git から受けた引数をそのまま渡す（引数の数を確かめるのはこちらの検査だけ）
    if let Some(local) = local {
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

/// cmd に input を標準入力で渡して実行し、終了コードと標準出力を返す（標準エラーは git に渡す）。終わりを受けられ
/// なければ、理由を出して Unfinished
fn run_with_input(
    cmd: &mut Command,
    input: &[u8],
    capture: bool,
    what: &str,
) -> Result<(i32, Vec<u8>), Unfinished> {
    if capture {
        cmd.stdout(Stdio::piped());
    }
    match crate::process::output_with_input(cmd, input) {
        // シグナルで終わったら（code が無い）失敗とする
        Ok(out) => Ok((out.status.code().unwrap_or(1), out.stdout)),
        Err(Unfinished::NotStarted(e)) => {
            eprintln!("❌ {what} を起動できません（{e}）");
            Err(Unfinished::NotStarted(e))
        }
        Err(Unfinished::Lost(e)) => {
            eprintln!("❌ {what} の終わりを待てません（{e}）");
            Err(Unfinished::Lost(e))
        }
    }
}

fn post_rewrite(args: &[String]) -> i32 {
    // 標準入力を読む前に確かめる（端末から引数を誤って呼ぶと、入力を待って止まる）
    let amend = match args {
        [kind] if kind == "amend" => true,
        [kind] if kind == "rebase" => false,
        _ => {
            eprintln!("{USAGE}");
            return 2;
        }
    };
    let mut input = Vec::new();
    if let Err(e) = std::io::stdin().read_to_end(&mut input) {
        eprintln!("❌ git が渡した旧と新のコミットの対応を読めません（{e}）");
        return 1;
    }
    let dir = Path::new(".");
    let Ok(local) = find_local_hook(dir, "post-rewrite") else {
        return 1;
    };
    // ローカルの hook の失敗でも付け直しは行い、その終了コードを最後に返す（ローカルの hook があるだけで付け直しが
    // 外れないように）
    let local_rc = match local {
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
    if amend && rebasing(dir) {
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
            Err(Unfinished::NotStarted(std::io::Error::from(
                std::io::ErrorKind::NotFound,
            )))
        }
    };
    // 貼って使えるよう、パスをシェルの引数の形にする（空白などを含むリポジトリのため）
    let retry = format!(
        "commit-refs remap {}",
        sh_quote(&saved.display().to_string())
    );
    let stdout = match out {
        Ok((0, stdout)) => stdout,
        failed => {
            if let Err(e) = std::fs::write(&saved, &input) {
                eprintln!("❌ 対応を {} に残せません（{e}）", saved.display());
            }
            match failed {
                Ok(_) => {
                    println!(
                        "❌ コミットの番号を付け直せませんでした（書き換えたかどうかは、上の ERROR の最後の行にあります）"
                    );
                    println!("   修正方法: 上の ERROR を直してから、{retry}");
                }
                Err(Unfinished::NotStarted(_)) => {
                    println!(
                        "❌ commit-refs を起動できなかったので、コミットの番号を付け直していません（何も書き換えていません）"
                    );
                    println!("   修正方法: commit-refs を起動できるようにしてから、{retry}");
                }
                Err(Unfinished::Lost(_)) => {
                    println!(
                        "❌ commit-refs の終わりを受けられなかったので、書き換えたかどうか分かりません"
                    );
                    println!(
                        "   修正方法: git diff で書き換えた文書を確かめ、書き換えていなければ {retry}"
                    );
                }
            }
            return 1;
        }
    };
    // commit-refs は REPLACED: のパスをバイト列のまま書く（UTF-8 とは限らない）ので、そのまま渡す
    let mut stdout_lock = std::io::stdout().lock();
    let _ = stdout_lock.write_all(&stdout);
    if stdout.last().is_some_and(|b| *b != b'\n') {
        let _ = stdout_lock.write_all(b"\n");
    }
    drop(stdout_lock);
    if !stdout.split(|b| *b == b'\n').any(|line| line == b"REFS: 0") {
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

/// ALLOW_SECRET_COMMIT の値。空白（Unicode の White_Space）とバックスラッシュを除いて空か 0 なら付けていないもの、
/// 理由の幅（crate::shell::reason_width）が足りなければ短いものとする。展開（$VAR）を含む値は、この hook には
/// 展開後の値しか見えないので、PreToolUse hook の側で止める
#[derive(PartialEq)]
enum Allow {
    Absent,
    Short,
    Reason,
}

fn allow_secret_commit() -> Allow {
    let value = std::env::var("ALLOW_SECRET_COMMIT").unwrap_or_default();
    let stripped: String = value
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '\\')
        .collect();
    if stripped.is_empty() || stripped == "0" {
        Allow::Absent
    } else if crate::shell::reason_width(&value) >= crate::shell::REASON_MIN_WIDTH {
        Allow::Reason
    } else {
        Allow::Short
    }
}

fn scan_secrets() -> i32 {
    if std::env::var("CLAUDECODE").as_deref() != Ok("1") {
        return 0;
    }
    let allow = allow_secret_commit();
    if allow == Allow::Reason {
        return 0;
    }
    // 短い値は、止めるときに本来の文言に理由を求める行を足す（しきい値は文言に出さない。長さに合わせた無意味な値を
    // 誘うため）
    let reason_required = || {
        if allow == Allow::Short {
            println!(
                "   ALLOW_SECRET_COMMIT には理由を記載してください（ALLOW_SECRET_COMMIT=<理由> の形で、値はそのまま記録に残ります）"
            );
        }
    };
    let Some(bin) = crate::tools::find(GITLEAKS, "gitleaks") else {
        println!(
            "❌ gitleaks が見つからないので、機密情報を検査できません。コミットを止めます（Linux は claude-hooks の package.nix が {GITLEAKS} を渡す）"
        );
        reason_required();
        return 1;
    };
    match crate::gitleaks::scan(&bin, Path::new("."), "--staged") {
        crate::gitleaks::Scan::Clean => 0,
        crate::gitleaks::Scan::Leaks(list) => {
            println!("❌ 機密情報らしき内容がコミット対象に含まれています（gitleaks）:");
            for leak in list {
                println!("- {leak}");
            }
            println!(
                "   対処: git restore --staged <file> で外し、必要なら .gitignore に足してください。push 済みの値はローテーションしてください"
            );
            println!(
                "   誤検出なら、その行に gitleaks:allow を書いてください（.gitleaksignore / .gitleaks.toml でも可）"
            );
            println!(
                "   どうしても外すときは、理由を値にして ALLOW_SECRET_COMMIT=<理由> を付けてください（理由はそのまま記録に残ります）。機械で判定できる理由なら hook の不足なので、issue に書いてください"
            );
            reason_required();
            1
        }
        crate::gitleaks::Scan::Missing(e) => {
            println!(
                "❌ {bin} を起動できないので、機密情報を検査できません。コミットを止めます（{e}）"
            );
            reason_required();
            1
        }
        crate::gitleaks::Scan::Failed(failure) => {
            println!(
                "❌ {}。機密情報を検査できないので、コミットを止めます:",
                failure.what
            );
            for line in failure.errors.lines() {
                println!("   {line}");
            }
            reason_required();
            1
        }
    }
}

/// main / master への直接のコミットを止めるときの文（PreToolUse hook の block-main-commit と同じ趣旨）
const MAIN_COMMIT_DENY: &str = "❌ main / master ブランチへの直接コミットは禁止されています
   featureブランチを作成してください: /git-branch
   - GitHub のリモートが無いリポジトリでも、ブランチは /git-merge で main にマージするので、main に直接コミットする理由はありません（2026-11-06 に、cd が失敗した後の git commit が別のリポジトリの main に空のコミットを作りました）
   - 対象のディレクトリを確かめる（前の cd が失敗すると、git は元のディレクトリのリポジトリで走ります）
   - 試すための一時のリポジトリなら、main / master 以外のブランチで作る（git init -b <名前>、git switch -c <名前>）
   - 通すのは、flake.lock だけの変更、マージを締めるコミット（MERGE_HEAD がある）、HEAD の無いリポジトリの最初のコミットです";

fn pre_commit(args: &[String]) -> i32 {
    let dir = Path::new(".");
    let claude_code = std::env::var("CLAUDECODE").as_deref() == Ok("1");
    if claude_code && crate::rules::main_commit_blocked(dir) {
        println!("{MAIN_COMMIT_DENY}");
        return 1;
    }
    // マージを締めるコミット（競合を解決した後の git commit / git merge --continue、git merge --no-commit の後の
    // git commit）では git が pre-merge-commit を呼ばないので、同じ検査をここで呼ぶ。隣の hook は core.hooksPath の
    // 置き場にあり、--git-path hooks/… がそこを指す
    if git::git(dir, &["rev-parse", "-q", "--verify", "MERGE_HEAD"]).is_some() {
        let Some(sibling) = git::git(dir, &["rev-parse", "--git-path", "hooks/pre-merge-commit"])
        else {
            println!(
                "❌ pre-merge-commit の hook の場所を git から得られないので、マージ結果を検査できません"
            );
            return 1;
        };
        let code = run_local(Path::new(&sibling), &[]);
        if code != 0 {
            return code;
        }
    }
    let Ok(local) = find_local_hook(dir, "pre-commit") else {
        return 1;
    };
    if let Some(local) = local {
        let code = run_local(&local, args);
        if code != 0 {
            return code;
        }
        return scan_secrets();
    }
    let mut failed = false;
    // プロジェクトの pre-commit の設定（設計の記録の検査などへのオプトイン。ADR-0009）。core.hooksPath があると
    // prek install は拒否されるので、ここから呼ぶ。設定はそのプロジェクトの規約なので、fork でも実行する
    if Path::new(".pre-commit-config.yaml").is_file() {
        match crate::staged_checks::find_on_path("prek") {
            Some(prek) => {
                let ran = Command::new(&prek)
                    .args(["run", "--hook-stage", "pre-commit"])
                    .args(args)
                    .status();
                match ran {
                    Ok(status) => failed |= !status.success(),
                    Err(e) => {
                        println!("❌ {} を起動できません（{e}）", prek.display());
                        failed = true;
                    }
                }
            }
            None => println!(
                "⚠️  .pre-commit-config.yaml がありますが prek が PATH に無いので、その検査を飛ばします"
            ),
        }
    }
    let staged = git::git(
        dir,
        &["diff", "--cached", "--name-only", "--diff-filter=ACMRT"],
    );
    let Some(staged) = staged else {
        println!("❌ ステージしたファイルを git から読めないので、検査できません");
        return 1;
    };
    // ステージしたファイルが無くても（種類の変更だけのコミット）、機密情報は検査する
    if !staged.is_empty() {
        // 他人のプロジェクトの fork では、こちらのツールの版と規約（ruff の新しいルール、markdownlint の設定、
        // rustfmt の edition）が上流の固定版と CI と食い違い、上流向けのコミットに無関係な差分を混ぜる
        if git::is_fork(dir) {
            println!(
                "⏭️  upstream リモートのある fork なので、こちらの規約の検査（Nix / Python / Markdown / Rust / 日本語の語）を飛ばします"
            );
        } else {
            failed |= crate::staged_checks::run(&[]) != 0;
            // 日本語の崩れた語（ADR-0014）。Markdown の自動修正がステージし直した後の、コミットに入る中身を見る
            failed |= ja_terms_staged() != 0;
        }
    }
    failed |= scan_secrets() != 0;
    i32::from(failed)
}

fn ja_terms_staged() -> i32 {
    let Some(bin) = ja_terms() else {
        println!(
            "❌ ja-terms が見つからないので、崩れた日本語の語を検査できません（Linux は claude-hooks の package.nix が CLAUDE_HOOKS_JA_TERMS を渡す）"
        );
        return 1;
    };
    status(Command::new(&bin).arg("staged"), &bin)
}

/// プロジェクトのローカルの hook（git::local_hook）。git の共通のディレクトリを求められなければ、理由を出して Err。
/// ローカルの hook を探せないまま通すと、プロジェクトの hook（Change-Id を足す commit-msg など）を飛ばすので止める
fn find_local_hook(dir: &Path, name: &str) -> Result<Option<std::path::PathBuf>, ()> {
    let Some(common) = git::git(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ) else {
        println!(
            "❌ git rev-parse --git-common-dir に失敗したので、ローカルの {name} を探せません（同じディレクトリで git rev-parse --path-format=absolute --git-common-dir を実行すると、git の理由が出ます）"
        );
        return Err(());
    };
    Ok(git::local_hook(Path::new(&common), name))
}

fn pre_merge_commit(args: &[String]) -> i32 {
    let dir = Path::new(".");
    let Ok(local) = find_local_hook(dir, "pre-merge-commit") else {
        return 1;
    };
    if let Some(local) = local {
        let code = run_local(&local, args);
        if code != 0 {
            return code;
        }
    }
    let scanned = scan_secrets();
    if scanned != 0 {
        return scanned;
    }
    let branch = git::current_branch(dir);
    if branch != "main" && branch != "master" {
        return 0;
    }
    if git::is_fork(dir) {
        println!(
            "⏭️  upstream リモートのある fork なので、マージ結果の品質チェック（run-checks.sh）を飛ばします"
        );
        return 0;
    }
    // 検査は作業ツリーで走るので、コミットされない変更があると、マージコミットとは別の内容を検査してしまう（比べる
    // index は git が渡す GIT_INDEX_FILE。commit -a なら index.lock）。追跡していないファイルも、木全体の検査が拾う
    // ので含める。.gitignore で除外したものは検査のツールも除外するので含めない。core.quotePath=false は、下の
    // .gitignore から外れた生成物の一覧と同じく、非 ASCII のパスを 8 進で引用せずに出すため
    let lines = |args: &[&str]| -> Option<Vec<String>> {
        git::git(dir, args).map(|out| {
            out.lines()
                .filter(|l| !l.is_empty())
                .map(str::to_string)
                .collect()
        })
    };
    let (Some(modified), Some(untracked)) = (
        lines(&["-c", "core.quotePath=false", "diff", "--name-only"]),
        lines(&[
            "-c",
            "core.quotePath=false",
            "ls-files",
            "--others",
            "--exclude-standard",
        ]),
    ) else {
        println!("❌ 作業ツリーの変更を git から読めないので、マージ結果を検査できません");
        return 1;
    };
    if !modified.is_empty() || !untracked.is_empty() {
        report_unclean(dir, &modified, &untracked);
        return 1;
    }
    let run_checks =
        crate::shell::home().join(".claude/skills/language-checks/scripts/run-checks.sh");
    if !run_checks.is_file() {
        println!(
            "❌ {} がないので、マージ結果を検査できません",
            run_checks.display()
        );
        println!(
            "   修正方法: claude-sync（または rebuild）で ~/.claude を同期してから、もう一度マージしてください"
        );
        return 1;
    }
    println!("🔍 マージ結果の品質チェック（run-checks.sh --merge）...");
    // git が hook に渡す GIT_INDEX_FILE（.git/index や index.lock）は、検査が起動するテストの中の git にも引き継がれ、
    // 別のリポジトリでこの index を読み書きさせてしまうので外す（--merge は HEAD と作業ツリーの差分で判定し、index
    // に頼らない）
    // Windows の起動は、PATH の前に System32 を探し、WSL の bash.exe（Linux のディストリビューションを起動するもの）を
    // 見つける。Git for Windows の bash を使うよう、PATH を順に探して見つけたパスで起動する
    let bash = crate::staged_checks::find_on_path("bash").unwrap_or_else(|| "bash".into());
    let checked = Command::new(&bash)
        .arg(&run_checks)
        .arg("--merge")
        .env_remove("GIT_INDEX_FILE")
        .status();
    let passed = match checked {
        Ok(status) => status.success(),
        Err(e) => {
            println!("❌ {} を起動できません（{e}）", bash.display());
            false
        }
    };
    if !passed {
        println!("❌ マージ結果が品質チェックに通らないので、マージコミットを作りません");
        println!(
            "   修正方法: git merge --abort で取り消し、マージするブランチで直してコミットしてから、もう一度マージしてください"
        );
        return 1;
    }
    0
}

/// 作業ツリーにマージ結果以外の変更があることを、ファイルの一覧と修正方法で示す。マージするブランチが .gitignore から
/// 行を外すと、HEAD の作業ツリーに残っていた生成物が未追跡として現れる。コミットするものではないので、HEAD の
/// .gitignore で無視されていたファイルを見分け、別の一覧で示す
fn report_unclean(dir: &Path, modified: &[String], untracked: &[String]) {
    // --no-renames: ディレクトリを移したブランチで、改名として新しい .gitignore だけを出すと、無視の行を持っていた
    // 元の .gitignore が落ちる
    let ignore_changed: Vec<String> = git::git(
        dir,
        &[
            "-c",
            "core.quotePath=false",
            "diff",
            "--cached",
            "--no-renames",
            "--name-only",
            "HEAD",
            "--",
            ":(glob)**/.gitignore",
        ],
    )
    .map(|out| {
        out.lines()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect()
    })
    .unwrap_or_default();
    let mut unignored = Vec::new();
    let mut mark_failed = false;
    if !ignore_changed.is_empty() && !untracked.is_empty() {
        match head_ignored(dir, untracked) {
            Ok(list) => unignored = list,
            Err(why) => {
                println!("❌ HEAD の .gitignore で無視されていたファイルを判定できません（{why}）");
                mark_failed = true;
            }
        }
    }
    // 印の付いたファイルは下の一覧にだけ出す（同じファイルを 2 度並べると、大きな出力のディレクトリで修正方法が
    // 埋もれる）
    let others: Vec<&String> = modified
        .iter()
        .chain(untracked)
        .filter(|f| !unignored.contains(f))
        .collect();
    println!("❌ 作業ツリーにマージ結果以外の変更があるので、マージ結果を検査できません:");
    for f in &others {
        println!("     {f}");
    }
    if !unignored.is_empty() {
        println!(
            "   次のファイルは、マージするブランチが .gitignore から外した生成物（HEAD の .gitignore では無視されていた。ブランチが生成する処理を消したか置き換えたなど）:"
        );
        for f in &unignored {
            println!("     {f}");
        }
        println!("   マージで変わる .gitignore:");
        for f in &ignore_changed {
            println!("     {f}");
        }
        println!(
            "   修正方法: git merge --abort で取り消す。上の生成物は、DVC のキャッシュなどに記録された版と中身が同じで戻せるなら作業ツリーから消す。記録された版と中身が違うか、記録が無ければ、消すと戻せないので、何の出力でなぜ違うかの原因を調べてから消すか移すかを決める（残して無視し続けるなら、マージするブランチの .gitignore に行を戻す）。ほかの変更はコミットして（不要なファイルは消して）から、もう一度マージしてください"
        );
        return;
    }
    if mark_failed {
        println!(
            "   マージで次の .gitignore が変わるが、HEAD の .gitignore で無視されていたファイルの判定に失敗した（理由は上のエラー）。上の一覧には、マージするブランチが .gitignore から外した生成物が混ざりうる:"
        );
        for f in &ignore_changed {
            println!("     {f}");
        }
    }
    println!(
        "   修正方法: git merge --abort で取り消し、変更をコミットして（不要なファイルは消して）から、もう一度マージしてください"
    );
}

/// untracked のうち、HEAD の .gitignore で無視されていたもの。git check-ignore は作業ツリー（マージ結果）の
/// .gitignore しか読まないので、HEAD の .gitignore だけを置いた一時のリポジトリで判定する（ファイルの実体は要らない。
/// パスとパターンで決まる）。git archive は export-ignore の属性で .gitignore を落としうるので、git show で 1 本ずつ
/// 取り出す
fn head_ignored(dir: &Path, untracked: &[String]) -> Result<Vec<String>, String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let tmp = std::env::temp_dir().join(format!(
        "claude-hooks-ignored-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir(&tmp).map_err(|e| format!("{} を作れません（{e}）", tmp.display()))?;
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(tmp.clone());
    let repo = tmp.join("repo");
    let tree = crate::git::run(dir, &["ls-tree", "-r", "-z", "--name-only", "HEAD"], None)
        .filter(|o| o.status.success())
        .ok_or("git ls-tree HEAD に失敗しました")?;
    for name in tree.stdout.split(|b| *b == 0).filter(|n| !n.is_empty()) {
        let name = String::from_utf8_lossy(name);
        if name != ".gitignore" && !name.ends_with("/.gitignore") {
            continue;
        }
        let text = crate::git::run(dir, &["show", &format!("HEAD:{name}")], None)
            .filter(|o| o.status.success())
            .ok_or_else(|| format!("git show HEAD:{name} に失敗しました"))?;
        let dest = repo.join(&*name);
        std::fs::create_dir_all(dest.parent().unwrap_or(&repo))
            .and_then(|()| std::fs::write(&dest, text.stdout))
            .map_err(|e| format!("{} を書けません（{e}）", dest.display()))?;
    }
    std::fs::create_dir_all(&repo).map_err(|e| e.to_string())?;
    // hook に渡る GIT_INDEX_FILE などが、一時のリポジトリの git にも効かないように外す
    let tmp_git = |args: &[&str], input: Option<&[u8]>| {
        let mut cmd = Command::new("git");
        cmd.arg("-C")
            .arg(&repo)
            .args(args)
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        crate::process::output_with_input(&mut cmd, input.unwrap_or_default())
            .map_err(|_| "一時のリポジトリで git を起動できません".to_string())
    };
    let init = tmp_git(&["init", "-q"], None)?;
    if !init.status.success() {
        return Err(String::from_utf8_lossy(&init.stderr).trim().to_string());
    }
    // ls-files と check-ignore は同じ C の引用（タブ、改行、"、\）で書き、読み戻すので、行のまま渡す
    let input = untracked.join("\n") + "\n";
    let out = tmp_git(
        &["-c", "core.quotePath=false", "check-ignore", "--stdin"],
        Some(input.as_bytes()),
    )?;
    // 終了値 1 は「どれも無視されない」
    if !matches!(out.status.code(), Some(0 | 1)) {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect())
}
