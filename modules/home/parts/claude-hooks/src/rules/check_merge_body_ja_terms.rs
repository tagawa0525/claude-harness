//! check-merge-body-ja-terms: `git merge -m/-F` の本文の崩れた日本語の語を、実行の前に検査する
//!
//! git の hook は pre-merge-commit（run-checks --merge、数分）→ commit-msg（textlint、数秒）の順に呼ばれる。
//! 本文に辞書（ja-terms の prh.yml）で崩れた語があると、数分の品質チェックを待ってから commit-msg が拒む。
//! pre-merge-commit からは `-m` の本文が読めず、git の hook の順も変えられないので、Claude のツール呼び出しの
//! 前に走るこのルールで、commit-msg が呼ぶのと同じ検査（ja-terms-message。modules/home/parts/ja-terms/
//! message.sh。検査する範囲と textlint の設定を持つ。ビルド時に store のパスを埋める）に本文を渡す（ADR-0050）。
//!
//! 対象は本文を持つ形（`-m` / `--message`、`-F` / `--file`）。本文の無い形（エディタ）は対象外。
//! commit-msg が検査しない所（fork のリポジトリ、ローカルの commit-msg に置き換わるリポジトリ）も対象外。
//! 検査を起動できないか、検査に失敗したときは、判定できなかった理由を利用者に見せて通す（ADR-0033。
//! commit-msg が同じ検査で止める）。エスケープは設けない（語を直せば通る）

use super::Rule;
use crate::git;
use crate::git_args::{Item, option_values};
use crate::input::Input;
use crate::output::{Finding, notice};
use crate::shell::{Arg, Shell};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub struct CheckMergeBodyJaTerms;

/// ビルド時に埋める ja-terms-message の store のパス（package.nix の env）。cargo で直接ビルドしたものには無い
const JA_TERMS_MESSAGE: Option<&str> = option_env!("CLAUDE_HOOKS_JA_TERMS_MESSAGE");

/// `git merge args…` の本文。`-m` の値（複数なら空行で連ねる）か、`-F` のファイル（dir からの相対）の内容。
/// 本文の無い形は Ok(None)、`-F` のファイルが読めなければ Err にパス
fn merge_message(args: &[Arg], dir: &Path) -> Result<Option<String>, String> {
    let mut parts = Vec::new();
    for (opt, value) in option_values("merge", args) {
        match opt {
            Item::Short('m') | Item::Long("--message") => parts.push(message_value(value)),
            Item::Short('F') | Item::Long("--file") => {
                match std::fs::read_to_string(message_path(value, dir, None)) {
                    Ok(text) => parts.push(text),
                    Err(_) => return Err(value.to_string()),
                }
            }
            _ => {}
        }
    }
    if parts.is_empty() {
        return Ok(None);
    }
    Ok(Some(parts.join("\n\n")))
}

/// `-F` のファイルのパス。`~/` は home の下、相対パスは dir の下
fn message_path(value: &str, dir: &Path, home: Option<&Path>) -> std::path::PathBuf {
    let _ = home;
    dir.join(value)
}

/// `-m` の値。ヒアドキュメントを `$(cat <<…)` で渡した形（`-m "$(cat <<'EOF' … EOF)"`）は本文を取り出し、
/// ほかは書いたまま（展開しない）
fn message_value(text: &str) -> String {
    heredoc_body(text).unwrap_or_else(|| text.to_string())
}

/// `$(cat <<[-]DELIM\n本文\nDELIM\n)` の本文。区切りのクォート（'EOF'、"EOF"）は外す。`<<-` は行の先頭の
/// タブを外す
fn heredoc_body(text: &str) -> Option<String> {
    let rest = text.strip_prefix("$(cat <<")?.strip_suffix(')')?;
    let (dash, rest) = match rest.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, rest),
    };
    let (head, body) = rest.split_once('\n')?;
    let delim = head.trim().trim_matches(|c| c == '\'' || c == '"');
    if delim.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    for line in body.lines() {
        let line = if dash {
            line.trim_start_matches('\t')
        } else {
            line
        };
        if line == delim {
            return Some(lines.join("\n"));
        }
        lines.push(line);
    }
    None
}

/// ja-terms-message（textlint）の結果
#[derive(Debug, PartialEq)]
enum Lint {
    Clean,
    /// 崩れた語がある。「誤り => 正しい語」の行
    Terms(Vec<String>),
    /// 検査を起動できない、または検査に失敗した。理由
    Failed(String),
}

/// commit-msg と同じく、メッセージを標準入力で ja-terms-message に渡す。commit-msg と同じくリポジトリで実行する
fn lint(check: &str, dir: &Path, message: &str) -> Lint {
    let child = Command::new(check)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return Lint::Failed(format!("{check} を起動できません（{e}）")),
    };
    // 本文は数 KB なので、書き終えてから出力を読んでもパイプは詰まらない
    if let Some(mut stdin) = child.stdin.take()
        && let Err(e) = stdin.write_all(message.as_bytes())
    {
        let _ = child.kill();
        return Lint::Failed(format!("{check} に本文を渡せません（{e}）"));
    }
    let out = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => return Lint::Failed(format!("{check} の終わりを待てません（{e}）")),
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    match out.status.code() {
        Some(0) => Lint::Clean,
        // prh の行（`  2:1  ✓ error  誤り => 正しい語  prh`）から「誤り => 正しい語」を取り出す
        Some(1) => Lint::Terms(
            stdout
                .lines()
                .filter(|l| l.contains(" => "))
                .map(|l| {
                    let l = l.trim_end().trim_end_matches("prh").trim();
                    l.split_once("error")
                        .map_or(l, |(_, rest)| rest)
                        .trim()
                        .to_string()
                })
                .collect(),
        ),
        _ => Lint::Failed(format!(
            "{check} が失敗しました（{}）: {}{}",
            out.status,
            stdout.trim(),
            String::from_utf8_lossy(&out.stderr).trim()
        )),
    }
}

/// 崩れた語を止める文言
fn deny_reason(terms: &[String]) -> String {
    let list: Vec<String> = terms.iter().map(|t| format!("  {t}")).collect();
    format!(
        "git merge の本文に、崩れた日本語の語があります（ja-terms の辞書）:\n{}\n\
         本文の語を「誤り => 正しい語」のとおりに置き換えてから、もう一度実行してください。このまま実行すると、\
         数分かかる pre-merge-commit の品質チェックの後で、commit-msg の hook が同じ検査で拒みます",
        list.join("\n")
    )
}

/// commit-msg が語を検査しないリポジトリか: `upstream` リモートのある fork と、ローカルの commit-msg に
/// 置き換わるリポジトリ（modules/home/parts/git.nix の commit-msg と同じ判定）
fn commit_msg_skips(dir: &Path) -> bool {
    if git::is_fork(dir) {
        return true;
    }
    use std::os::unix::fs::PermissionsExt;
    git::git(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .and_then(|common| std::fs::metadata(Path::new(&common).join("hooks/commit-msg")).ok())
    .is_some_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

impl CheckMergeBodyJaTerms {
    /// `check` は埋めた ja-terms-message のパス（無ければ判定できない）。`global_hooks` は dir で見た
    /// グローバルの core.hooksPath
    fn check_with(
        &self,
        input: &Input,
        shell: &Shell,
        check: Option<&str>,
        global_hooks: &dyn Fn(&Path) -> Option<String>,
    ) -> Vec<Finding> {
        let _ = global_hooks;
        let mut out = Vec::new();
        for cmd in shell.commands() {
            let Some(("merge", args)) = cmd.git_subcommand() else {
                continue;
            };
            let dir = shell.target_dir(cmd, &input.cwd);
            let message = match merge_message(args, &dir) {
                Ok(Some(m)) => m,
                Ok(None) => continue,
                // `-F -`（標準入力）や読めないパスは、git merge 自身が読むか失敗する
                Err(path) => {
                    notice(format!(
                        "check-merge-body-ja-terms: git merge -F {path} を読めないので、本文の語を判定していません"
                    ));
                    continue;
                }
            };
            if commit_msg_skips(&dir) {
                continue;
            }
            let Some(check) = check else {
                notice(
                    "check-merge-body-ja-terms: このビルドには ja-terms-message が埋め込まれていないので、git merge の本文の語を判定していません（nix でビルドしたものだけが検査する）".to_string(),
                );
                continue;
            };
            match lint(check, &dir, &message) {
                Lint::Clean => {}
                Lint::Terms(terms) => out.push(Finding::Deny(deny_reason(&terms))),
                Lint::Failed(why) => notice(format!(
                    "check-merge-body-ja-terms: git merge の本文の語を判定していません: {why}"
                )),
            }
        }
        out
    }
}

impl Rule for CheckMergeBodyJaTerms {
    fn name(&self) -> &'static str {
        "check-merge-body-ja-terms"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        self.check_with(input, shell, JA_TERMS_MESSAGE, &|dir| {
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
                "claude-hooks-merge-body-{tag}-{}-{seq}",
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

    /// ja-terms-message の代わり。引数なしで呼ばれたときだけ、標準入力の「誤語」を prh の形（誤語 => 正語）で
    /// 報告して 1 で、「失敗」を含めば 2 で終わる。辞書の実物の語はコミットの語の検査に掛かるので使わない
    /// （実物の ja-terms-message と辞書は tests/merge-ja-terms.sh が確かめる）
    const FAKE_CHECK: &str = r#"#!/bin/sh
[ $# -eq 0 ] || exit 3
body=$(cat)
case "$body" in
  *失敗*) echo "boom" >&2; exit 2 ;;
  *無語*) exit 1 ;;
  *誤語*)
    printf '%s\n' "COMMIT_EDITMSG.txt" "  2:1  ✓ error  誤語 => 正語  prh" "" "✖ 1 problem (1 error, 0 warnings, 0 infos)"
    exit 1 ;;
esac
exit 0
"#;

    fn fake_check(dir: &TempDir) -> String {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.0.join("ja-terms-message");
        std::fs::write(&path, FAKE_CHECK).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().into_owned()
    }

    /// テストのグローバルの core.hooksPath。git_init のリポジトリはこれをローカルに持つ（手元とサンドボックスで
    /// グローバルの設定が違っても、グローバルの hook を使うリポジトリにする）
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

    fn args(src: &str) -> Vec<Arg> {
        let cs = Shell::parse(src).commands().to_vec();
        let (_, args) = cs[0].git_subcommand().unwrap();
        args.to_vec()
    }

    fn message(src: &str) -> Result<Option<String>, String> {
        merge_message(&args(src), Path::new("."))
    }

    fn check_in(command: &str, cwd: &Path, check: Option<&str>) -> Vec<Finding> {
        let json = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": command},
            "cwd": cwd,
        });
        let input = Input::parse(&json.to_string()).unwrap();
        CheckMergeBodyJaTerms.check_with(&input, &Shell::parse(command), check, &global_hooks)
    }

    /// git のリポジトリの中で、偽の ja-terms-message で検査する
    fn check(command: &str) -> Vec<Finding> {
        let dir = TempDir::new("check");
        git_init(&dir.0);
        let check = fake_check(&dir);
        check_in(command, &dir.0, Some(&check))
    }

    fn deny_reason(findings: &[Finding]) -> String {
        match findings {
            [Finding::Deny(r)] => r.clone(),
            _ => panic!("deny が 1 件のはず"),
        }
    }

    #[test]
    fn message_from_m_option() {
        assert_eq!(
            message("git merge --no-ff feat/a -m 'Merge: a'"),
            Ok(Some("Merge: a".to_string()))
        );
    }

    #[test]
    fn message_from_attached_and_long_forms() {
        assert_eq!(
            message("git merge -m'Merge: a' feat/a"),
            Ok(Some("Merge: a".to_string()))
        );
        assert_eq!(
            message("git merge --message='Merge: a' feat/a"),
            Ok(Some("Merge: a".to_string()))
        );
        assert_eq!(
            message("git merge --message 'Merge: a' feat/a"),
            Ok(Some("Merge: a".to_string()))
        );
    }

    #[test]
    fn message_joins_multiple_m_options_as_paragraphs() {
        assert_eq!(
            message("git merge -m 'Merge: a' -m '本文の語' feat/a"),
            Ok(Some("Merge: a\n\n本文の語".to_string()))
        );
    }

    #[test]
    fn message_reads_heredoc_body_in_m_option() {
        assert_eq!(
            message(
                "git merge --no-ff feat/a -m \"$(cat <<'EOF'\nMerge: a\n\n## Why\n誤語\nEOF\n)\""
            ),
            Ok(Some("Merge: a\n\n## Why\n誤語".to_string()))
        );
        assert_eq!(
            message("git merge -m \"$(cat <<EOF\nMerge: a\n\n本文\nEOF\n)\" feat/a"),
            Ok(Some("Merge: a\n\n本文".to_string()))
        );
    }

    #[test]
    fn message_absent_without_m_or_f() {
        assert_eq!(message("git merge --no-ff feat/a"), Ok(None));
        assert_eq!(message("git merge --continue"), Ok(None));
    }

    #[test]
    fn message_reads_file_relative_to_dir() {
        let dir = TempDir::new("file");
        std::fs::write(dir.0.join("msg.txt"), "Merge: a\n\n本文\n").unwrap();
        for src in [
            "git merge -F msg.txt feat/a",
            "git merge --file=msg.txt feat/a",
        ] {
            assert_eq!(
                merge_message(&args(src), &dir.0),
                Ok(Some("Merge: a\n\n本文\n".to_string())),
                "{src}"
            );
        }
    }

    #[test]
    fn message_reports_unreadable_file() {
        assert_eq!(
            message("git merge -F /nonexistent/msg.txt feat/a"),
            Err("/nonexistent/msg.txt".to_string())
        );
    }

    #[test]
    fn lint_classifies_check_results() {
        let dir = TempDir::new("lint");
        let t = fake_check(&dir);
        assert_eq!(lint(&t, &dir.0, "確認\n"), Lint::Clean);
        assert_eq!(
            lint(&t, &dir.0, "誤語\n"),
            Lint::Terms(vec!["誤語 => 正語".to_string()])
        );
        let Lint::Failed(why) = lint(&t, &dir.0, "失敗\n") else {
            panic!("終了コード 2 は失敗のはず");
        };
        assert!(why.contains("boom"), "{why}");
        assert!(matches!(
            lint("/nonexistent/ja-terms-message", &dir.0, "確認\n"),
            Lint::Failed(_)
        ));
    }

    #[test]
    fn denies_broken_term_with_correction_and_reason() {
        let r = deny_reason(&check(
            "git merge --no-ff feat/a -m 'Merge: a' -m '誤語する'",
        ));
        assert!(r.contains("誤語 => 正語"), "{r}");
        // 理由（数分後に commit-msg で拒まれる）と修正方法（語の置き換え）
        assert!(r.contains("commit-msg"), "{r}");
        assert!(r.contains("置き換え"), "{r}");
    }

    #[test]
    fn denies_broken_term_in_heredoc_message() {
        let r = deny_reason(&check(
            "git merge --no-ff feat/a -m \"$(cat <<'EOF'\nMerge: a\n\n## Why\n誤語\nEOF\n)\"",
        ));
        assert!(r.contains("誤語 => 正語"), "{r}");
    }

    #[test]
    fn denies_broken_term_in_message_file_under_cd() {
        let dir = TempDir::new("cd");
        git_init(&dir.0);
        let check = fake_check(&dir);
        std::fs::write(dir.0.join("msg.txt"), "Merge: a\n\n誤語\n").unwrap();
        let cmd = format!(
            "cd {} && git merge --no-ff -F msg.txt feat/a",
            dir.0.display()
        );
        let findings = check_in(&cmd, Path::new("/"), Some(&check));
        assert!(deny_reason(&findings).contains("誤語 => 正語"));
    }

    #[test]
    fn allows_clean_message() {
        assert!(check("git merge --no-ff feat/a -m 'Merge: a' -m '確認'").is_empty());
    }

    #[test]
    fn ignores_merge_without_message() {
        assert!(check("git merge --no-ff feat/a").is_empty());
    }

    #[test]
    fn ignores_other_commands_with_broken_terms() {
        assert!(check("echo 誤語 && git commit -m 'feat: 誤語'").is_empty());
    }

    #[test]
    fn ignores_fork_like_commit_msg() {
        let dir = TempDir::new("fork");
        git_init(&dir.0);
        let ok = Command::new("git")
            .args(["remote", "add", "upstream", "https://example.invalid/x.git"])
            .current_dir(&dir.0)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let check = fake_check(&dir);
        let findings = check_in(
            "git merge -m 'Merge: a' -m '誤語' feat/a",
            &dir.0,
            Some(&check),
        );
        assert!(findings.is_empty());
    }

    #[test]
    fn ignores_repository_with_local_commit_msg_hook() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new("local-hook");
        git_init(&dir.0);
        let hook = dir.0.join(".git/hooks/commit-msg");
        std::fs::write(&hook, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        let check = fake_check(&dir);
        let findings = check_in(
            "git merge -m 'Merge: a' -m '誤語' feat/a",
            &dir.0,
            Some(&check),
        );
        assert!(findings.is_empty());
    }

    #[test]
    fn reports_undecidable_when_check_is_not_embedded() {
        let dir = TempDir::new("none");
        git_init(&dir.0);
        let findings = check_in("git merge -m 'Merge: a' -m '誤語' feat/a", &dir.0, None);
        assert!(findings.is_empty());
        assert!(
            take_notices()
                .iter()
                .any(|n| n.contains("ja-terms-message")),
            "判定できなかった理由を出すはず"
        );
    }

    #[test]
    fn message_path_expands_home_and_joins_relative() {
        let home = Path::new("/home/u");
        assert_eq!(
            message_path("~/m.txt", Path::new("/r"), Some(home)),
            PathBuf::from("/home/u/m.txt")
        );
        assert_eq!(
            message_path("m.txt", Path::new("/r"), Some(home)),
            PathBuf::from("/r/m.txt")
        );
        assert_eq!(
            message_path("/abs/m.txt", Path::new("/r"), Some(home)),
            PathBuf::from("/abs/m.txt")
        );
    }

    #[test]
    fn reports_stdin_message_file_as_undecidable() {
        assert!(check("git merge --no-ff -F - feat/a").is_empty());
        assert!(
            take_notices().iter().any(|n| n.contains("標準入力")),
            "判定できなかった理由を出すはず"
        );
    }

    #[test]
    fn lint_without_terms_is_failure() {
        let dir = TempDir::new("no-terms");
        let t = fake_check(&dir);
        assert!(matches!(lint(&t, &dir.0, "無語\n"), Lint::Failed(_)));
    }

    #[test]
    fn ignores_merge_that_makes_no_commit() {
        assert!(check("git merge --ff-only feat/a -m 'Merge: a' -m '誤語'").is_empty());
        assert!(check("git merge --squash feat/a -m 'Merge: a' -m '誤語'").is_empty());
    }

    #[test]
    fn ignores_repository_with_its_own_hooks_path() {
        let dir = TempDir::new("own-hooks");
        git_init(&dir.0);
        git(&dir.0, &["config", "core.hooksPath", ".githooks"]);
        let check = fake_check(&dir);
        let findings = check_in(
            "git merge -m 'Merge: a' -m '誤語' feat/a",
            &dir.0,
            Some(&check),
        );
        assert!(findings.is_empty());
    }

    #[test]
    fn ignores_when_no_global_hooks_path() {
        let dir = TempDir::new("no-global");
        git_init(&dir.0);
        let check = fake_check(&dir);
        let json = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "git merge -m 'Merge: a' -m '誤語' feat/a"},
            "cwd": dir.0,
        });
        let input = Input::parse(&json.to_string()).unwrap();
        let command = "git merge -m 'Merge: a' -m '誤語' feat/a";
        let findings =
            CheckMergeBodyJaTerms
                .check_with(&input, &Shell::parse(command), Some(&check), &|_| None);
        assert!(findings.is_empty());
    }
}
