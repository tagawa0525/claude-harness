//! check-commit-message: `git commit -m/-F` のメッセージを、実行の前に commit-msg の hook と同じ検査で判定する
//!
//! commit-msg（modules/home/parts/git.nix）は、件名が Conventional Commits でないか 72 文字を超えるか、本文に
//! 崩れた日本語の語があるコミットを拒む。git の順で pre-commit（マージを終える commit では数分の品質チェック）の
//! 後に走り、拒まれても同じコマンドの `;` や改行の後のコマンドは古い HEAD のまま走る。pre-commit からは `-m` の
//! 本文が読めず、git の hook の順も変えられないので、Claude のツール呼び出しの前に走るこのルールで、commit-msg が
//! 呼ぶのと同じスクリプト（commit-msg-check。modules/home/parts/ja-terms/commit-msg-check.sh。store のパスは
//! package.nix が包んで渡す）に、commit-msg に渡るのと同じメッセージを渡す（ADR-0073）。
//!
//! 対象は本文を持つ形（`-m` / `--message`、`-F` / `--file`）。commit-msg を呼ばない形（`-n` / `--no-verify`、
//! `--dry-run`）、ほかのコミットからメッセージを作る形（`-c` / `-C`、`--fixup`、`--squash`）、commit-msg が検査しない
//! 所（[`commit_msg_skips`]）は対象外。判定できないとき（シェルが展開する本文、読めないファイル、ヒアドキュメントの
//! 無い `-F -`、検査を起動できないか検査が失敗した）は、理由を利用者に見せて通す（ADR-0033。commit-msg が同じ検査で
//! 止める）。同じコマンドの前段が `-F` のファイルを書きうるときは、書いた後の中身を判定できないので止める（#489）。
//! エスケープは設けない（メッセージを直せば通る）

use super::Rule;
use super::check_merge_body_ja_terms::{Undecided, commit_msg_skips, literal, split_calls_reason};
use crate::git;
use crate::git_args::{Item, items_with_values};
use crate::input::Input;
use crate::output::{Finding, notice};
use crate::shell::{self, Arg, Cmd, Shell};
use std::path::Path;

pub struct CheckCommitMessage;

/// commit-msg-check の store のパスを持つ環境変数。package.nix の包む側が固定する（cargo で直接ビルドした
/// ものには無い）
const COMMIT_MSG_CHECK: &str = "CLAUDE_HOOKS_COMMIT_MSG_CHECK";

/// メッセージを判定できない理由
#[derive(Debug, PartialEq)]
pub(super) enum Unknown {
    /// `-m` / `-F` の値（git merge と同じ読み方）
    Value(Undecided),
    /// 標準入力のメッセージ（`-F -`）を、同じコマンドのヒアドキュメントから取れない
    Stdin,
}

/// `git commit args…` のメッセージを、commit-msg に渡る形で組み立てる。本文の無い形と、git が止める形
/// （`-m` と `-F` の両方）は Ok(None)
pub(super) fn commit_message(
    cmd: &Cmd,
    args: &[Arg],
    dir: &Path,
) -> Result<Option<String>, Unknown> {
    let _ = (cmd, args, dir);
    Ok(None)
}

impl CheckCommitMessage {
    /// `check` は commit-msg-check のパス（無ければ判定できない）。`global_hooks` は dir で見た
    /// グローバルの core.hooksPath
    fn check_with(
        &self,
        input: &Input,
        shell: &Shell,
        check: Option<&str>,
        global_hooks: &dyn Fn(&Path) -> Option<String>,
    ) -> Vec<Finding> {
        let _ = (input, shell, check, global_hooks);
        Vec::new()
    }
}

impl Rule for CheckCommitMessage {
    fn name(&self) -> &'static str {
        "check-commit-message"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        let check = std::env::var(COMMIT_MSG_CHECK).ok();
        self.check_with(input, shell, check.as_deref(), &|dir| {
            git::git(dir, &["config", "--global", "--get", "core.hooksPath"])
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::take_notices;
    use std::path::PathBuf;
    use std::process::Command;

    /// テストの間だけある一時のディレクトリ（テストが panic しても消す）。テストは並行に走るので、同じ tag でも
    /// 呼ぶたびに別のディレクトリにする
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "claude-hooks-commit-message-{tag}-{}-{seq}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// commit-msg-check の代わり。メッセージのファイルを 1 つ受け、隣の `seen` にコピーする。件名（最初の行）に
    /// 「長すぎ」があれば commit-msg の形の ❌ を出して 1 で、「失敗」があれば 2 で終わる（規則の実物は
    /// tests/commit-message.sh と modules/home/parts/tests/commit-msg-conventions.sh が確かめる）
    const FAKE_CHECK: &str = r#"#!/bin/sh
[ $# -eq 1 ] || exit 3
cp "$1" "$(dirname "$0")/seen"
case "$(head -n 1 "$1")" in
  *長すぎ*) echo "❌ 件名が 72 文字を超えています (73 文字): $(head -n 1 "$1")"; exit 1 ;;
  *失敗*) echo "boom" >&2; exit 2 ;;
esac
exit 0
"#;

    struct Fake {
        dir: TempDir,
        path: String,
    }

    impl Fake {
        fn new(tag: &str) -> Fake {
            use std::os::unix::fs::PermissionsExt;
            let dir = TempDir::new(tag);
            git_init(&dir.0);
            let path = dir.0.join("commit-msg-check");
            std::fs::write(&path, FAKE_CHECK).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            // 並行するテストが子を fork した直後は、書き込みに開いた fd が exec までその子に残り、実行が
            // ETXTBSY（Text file busy）で失敗する。引数 2 つ（偽の検査は 3 で終わる）で起動できるまで待つ
            for _ in 0..100 {
                match Command::new(&path).args(["probe", "x"]).output() {
                    Err(e) if e.raw_os_error() == Some(26) => {
                        std::thread::sleep(std::time::Duration::from_millis(10))
                    }
                    _ => {
                        return Fake {
                            path: path.to_string_lossy().into_owned(),
                            dir,
                        };
                    }
                }
            }
            panic!(
                "偽の検査が 1 秒たっても Text file busy のまま: {}",
                path.display()
            );
        }

        /// リポジトリ（偽の検査の隣）で検査する
        fn check(&self, command: &str) -> Vec<Finding> {
            check_in(command, &self.dir.0, Some(&self.path))
        }

        /// 偽の検査が受け取ったメッセージ
        fn seen(&self) -> Option<String> {
            std::fs::read_to_string(self.dir.0.join("seen")).ok()
        }
    }

    /// 判定できない理由（output::notice）はプロセスに 1 つの置き場に集まり、take_notices は全部を取り出す。
    /// 理由を確かめるテストは、互いの理由を取り合わないように順に実行する
    static NOTICES: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// テストのグローバルの core.hooksPath。git_init のリポジトリはこれをローカルに持つ
    const GLOBAL_HOOKS: &str = "/nonexistent/global-hooks";

    fn global_hooks(_: &Path) -> Option<String> {
        Some(GLOBAL_HOOKS.to_string())
    }

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    fn git_init(dir: &Path) {
        git(dir, &["init", "-q"]);
        git(dir, &["config", "core.hooksPath", GLOBAL_HOOKS]);
    }

    fn check_in(command: &str, cwd: &Path, check: Option<&str>) -> Vec<Finding> {
        let json = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": command},
            "cwd": cwd,
        });
        let input = Input::parse(&json.to_string()).unwrap();
        CheckCommitMessage.check_with(&input, &Shell::parse(command), check, &global_hooks)
    }

    /// コマンドの最初の git commit のメッセージ（dir で読む）
    fn message_in(src: &str, dir: &Path) -> Result<Option<String>, Unknown> {
        let shell = Shell::parse(src);
        let cmd = shell
            .commands()
            .iter()
            .find(|c| matches!(c.git_subcommand(), Some(("commit", _))))
            .unwrap();
        let (_, args) = cmd.git_subcommand().unwrap();
        commit_message(cmd, args, dir)
    }

    fn message(src: &str) -> Result<Option<String>, Unknown> {
        message_in(src, Path::new("."))
    }

    fn some(s: &str) -> Result<Option<String>, Unknown> {
        Ok(Some(s.to_string()))
    }

    fn only_deny(findings: &[Finding]) -> String {
        match findings {
            [Finding::Deny(r)] => r.clone(),
            _ => panic!("deny が 1 件のはず"),
        }
    }

    /// xlc の記録にある件名（73 文字。2026-11-06 00:18 JST に commit-msg が拒み、後段の品質チェックが古い HEAD で
    /// 通ったと出した）の代わりに、偽の検査が拒む件名
    const LONG: &str = "docs(plans): 長すぎる件名";

    #[test]
    fn message_joins_m_options_as_paragraphs_like_git() {
        assert_eq!(
            message("git commit -m 'feat: a' -m 'b'"),
            some("feat: a\n\nb\n")
        );
        assert_eq!(message("git commit -m'feat: a'"), some("feat: a\n"));
        assert_eq!(message("git commit --message='feat: a'"), some("feat: a\n"));
        assert_eq!(
            message("git commit -m 'x' --no-message -m 'feat: a'"),
            some("feat: a\n")
        );
    }

    #[test]
    fn message_strips_spaces_like_git_before_commit_msg() {
        // git 2.55 で、ローカルの commit-msg に渡ったファイルを見て確かめた形（2026-11-06）: 行末の空白、先頭と末尾の
        // 空行を除き、続く空行を 1 つにする。# の行は残す
        assert_eq!(
            message("git commit -m '\n\nfeat: a  \n\n\n\nbody\t\n\n# c\n'"),
            some("feat: a\n\nbody\n\n# c\n")
        );
        assert_eq!(
            message("git commit --cleanup=verbatim -m '\nfeat: v  '"),
            some("\nfeat: v  \n")
        );
        assert_eq!(
            message("git commit --cleanup=strip -m 'feat: a\n# c\nb'"),
            some("feat: a\nb\n")
        );
    }

    #[test]
    fn message_reads_heredoc_in_m_option() {
        assert_eq!(
            message("git commit -m \"$(cat <<'EOF'\nfeat: a\n\nbody $X\nEOF\n)\""),
            some("feat: a\n\nbody $X\n")
        );
    }

    #[test]
    fn message_reads_stdin_heredoc_for_file_dash() {
        assert_eq!(
            message("git add a && git commit -q -F - <<'EOF'\nfeat: a\n\nbody $X\nEOF\ngit log -1"),
            some("feat: a\n\nbody $X\n")
        );
        assert_eq!(
            message("git commit --file=- <<EOF\nfeat: a\nEOF\n"),
            some("feat: a\n")
        );
    }

    #[test]
    fn message_from_stdin_is_undecided_without_a_literal_heredoc() {
        assert_eq!(
            message("printf 'feat: a\\n' | git commit -F -"),
            Err(Unknown::Stdin)
        );
        assert_eq!(
            message("git commit -F - <<EOF\nfeat: $X\nEOF\n"),
            Err(Unknown::Value(Undecided::Expanded("-F -")))
        );
    }

    #[test]
    fn message_reads_file_relative_to_dir() {
        let dir = TempDir::new("file");
        std::fs::write(dir.0.join("msg.txt"), "\nfeat: x   \nbody\n\n\n").unwrap();
        assert_eq!(
            message_in("git commit -F msg.txt", &dir.0),
            some("feat: x\nbody\n")
        );
        assert_eq!(
            message("git commit -F /nonexistent/msg.txt"),
            Err(Unknown::Value(Undecided::Unreadable(
                "/nonexistent/msg.txt".to_string()
            )))
        );
    }

    #[test]
    fn message_absent_for_forms_without_own_message() {
        for src in [
            "git commit",
            "git commit -a",
            "git commit -m 'feat: a' -F msg.txt",
            "git commit -C HEAD -m 'feat: a'",
            "git commit --fixup=HEAD -m 'feat: a'",
            "git commit --squash HEAD -m 'feat: a'",
            "git commit -c HEAD",
            "git commit -n -m 'feat: a'",
            "git commit --no-verify -m 'feat: a'",
            "git commit --dry-run -m 'feat: a'",
        ] {
            assert_eq!(message(src), Ok(None), "{src}");
        }
        assert_eq!(
            message("git commit --no-verify --verify -m 'feat: a'"),
            some("feat: a\n")
        );
    }

    #[test]
    fn denies_long_subject_in_stdin_heredoc_with_reason() {
        let fake = Fake::new("stdin");
        let cmd = format!(
            "git add docs/plans/m4-depletion.md && git commit -q -F - <<'EOF'\n{LONG}\n\n本文\n\nRefs: #0190\nEOF\ngit log --oneline -1 && run-checks.sh; echo \"rc=$?\""
        );
        let r = only_deny(&fake.check(&cmd));
        assert_eq!(
            fake.seen().unwrap(),
            format!("{LONG}\n\n本文\n\nRefs: #0190\n")
        );
        // 検査の出力と、理由（後段が古い HEAD で走る）と修正方法
        assert!(r.contains("❌ 件名が 72 文字を超えています"), "{r}");
        assert!(r.contains("古い HEAD"), "{r}");
        assert!(r.contains("直してから"), "{r}");
    }

    #[test]
    fn denies_long_subject_in_m_heredoc() {
        let fake = Fake::new("mheredoc");
        let cmd =
            format!("git commit -q -m \"$(cat <<'EOF'\n{LONG}\n\n本文\nEOF\n)\" && git log -1");
        assert!(only_deny(&fake.check(&cmd)).contains("❌"));
    }

    #[test]
    fn checks_commit_under_cd_and_git_c() {
        let fake = Fake::new("dirs");
        let d = fake.dir.0.display();
        for cmd in [
            format!("cd {d} && git commit -m '{LONG}'"),
            format!("git -C {d} commit -m '{LONG}'"),
        ] {
            let findings = check_in(&cmd, Path::new("/"), Some(&fake.path));
            assert!(only_deny(&findings).contains("❌"), "{cmd}");
        }
    }

    #[test]
    fn checks_commit_that_concludes_a_merge() {
        // マージの衝突の後に git commit で終える形（MERGE_HEAD がある）も、同じ commit-msg が走る
        let fake = Fake::new("merge-head");
        std::fs::write(fake.dir.0.join(".git/MERGE_HEAD"), "0".repeat(40) + "\n").unwrap();
        assert!(only_deny(&fake.check(&format!("git commit -m '{LONG}'"))).contains("❌"));
    }

    #[test]
    fn allows_message_the_check_passes() {
        let _guard = NOTICES.lock().unwrap_or_else(|e| e.into_inner());
        take_notices();
        let fake = Fake::new("ok");
        assert!(
            fake.check("git commit -q -m 'feat: a' && git log -1")
                .is_empty()
        );
        assert_eq!(fake.seen().unwrap(), "feat: a\n");
        assert!(take_notices().is_empty());
    }

    #[test]
    fn ignores_commit_without_message_and_other_commands() {
        let fake = Fake::new("none");
        assert!(fake.check("git commit").is_empty());
        assert!(
            fake.check(&format!("git merge --no-ff feat/a -m '{LONG}'"))
                .is_empty()
        );
        assert!(
            fake.check(&format!("echo \"git commit -m '{LONG}'\""))
                .is_empty()
        );
        assert_eq!(fake.seen(), None);
    }

    #[test]
    fn ignores_repositories_where_commit_msg_does_not_check() {
        let fake = Fake::new("fork");
        git(
            &fake.dir.0,
            &["remote", "add", "upstream", "https://example.invalid/x.git"],
        );
        assert!(fake.check(&format!("git commit -m '{LONG}'")).is_empty());
        let other = Fake::new("own-hooks");
        git(&other.dir.0, &["config", "core.hooksPath", ".githooks"]);
        assert!(other.check(&format!("git commit -m '{LONG}'")).is_empty());
        assert_eq!(fake.seen(), None);
        assert_eq!(other.seen(), None);
    }

    #[test]
    fn reports_undecidable_message_and_check() {
        let _guard = NOTICES.lock().unwrap_or_else(|e| e.into_inner());
        let fake = Fake::new("undecided");
        for (cmd, why) in [
            ("git commit -m \"$MSG\"", "-m の値はシェルが展開する"),
            ("printf x | git commit -F -", "-F - の標準入力"),
            (
                "git commit -F /nonexistent/m.txt",
                "-F /nonexistent/m.txt を読めない",
            ),
            ("git commit -m 'feat: 失敗'", "boom"),
        ] {
            take_notices();
            assert!(fake.check(cmd).is_empty(), "{cmd}");
            let notices = take_notices();
            assert!(
                notices
                    .iter()
                    .any(|n| n.contains("check-commit-message") && n.contains(why)),
                "{cmd}: {notices:?}"
            );
        }
        take_notices();
        assert!(check_in(&format!("git commit -m '{LONG}'"), &fake.dir.0, None).is_empty());
        assert!(
            take_notices()
                .iter()
                .any(|n| n.contains("CLAUDE_HOOKS_COMMIT_MSG_CHECK"))
        );
    }

    #[test]
    fn denies_message_file_written_earlier_in_the_same_command() {
        let fake = Fake::new("written");
        std::fs::write(fake.dir.0.join("msg.txt"), "feat: 前の中身\n").unwrap();
        for prefix in [
            "cat > msg.txt <<'EOF'\nfeat: a\nEOF\n",
            "sed -i 's/a/b/' ./msg.txt && ",
            "printf 'feat: a\\n' >> msg.txt; ",
        ] {
            let cmd = format!("{prefix}git commit -F msg.txt");
            let r = only_deny(&fake.check(&cmd));
            assert!(r.contains("分けて"), "{cmd}: {r}");
            assert!(r.contains("-F -"), "{cmd}: {r}");
        }
        assert_eq!(fake.seen(), None);
    }

    #[test]
    fn reads_message_file_that_no_earlier_command_writes() {
        let fake = Fake::new("unwritten");
        std::fs::write(fake.dir.0.join("msg.txt"), "feat: a\n").unwrap();
        assert!(
            fake.check("git add a.txt > log.txt && git commit -F msg.txt")
                .is_empty()
        );
        assert_eq!(fake.seen().unwrap(), "feat: a\n");
    }
}
