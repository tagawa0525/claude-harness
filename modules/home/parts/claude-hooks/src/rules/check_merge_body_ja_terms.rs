//! check-merge-body-ja-terms: `git merge -m/-F` の本文の崩れた日本語の語を、実行の前に検査する
//!
//! git の hook は pre-merge-commit（run-checks --merge、数分）→ commit-msg（textlint、数秒）の順に呼ばれる。
//! 本文に辞書（ja-terms の prh.yml）で崩れた語があると、数分の品質チェックを待ってから commit-msg が拒む。
//! pre-merge-commit からは `-m` の本文が読めず、git の hook の順も変えられないので、Claude のツール呼び出しの
//! 前に走るこのルールで、commit-msg と同じ textlint と設定（modules/home/parts/ja-terms/textlint.nix。
//! ビルド時に store のパスを埋める）を、commit-msg と同じ範囲の本文に当てる（ADR-0050）。
//!
//! 対象は本文を持つ形（`-m` / `--message`、`-F` / `--file`）。本文の無い形（エディタ）は対象外。
//! commit-msg が検査しない所（fork のリポジトリ、ローカルの commit-msg に置き換わるリポジトリ）も対象外。
//! textlint を起動できないか、検査に失敗したときは、判定できなかった理由を利用者に見せて通す（ADR-0033。
//! commit-msg が同じ検査で止める）。エスケープは設けない（語を直せば通る）

use super::Rule;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Arg, Shell};
use std::path::Path;

pub struct CheckMergeBodyJaTerms;

/// ビルド時に埋める textlint の実行ファイルと設定の store のパス（package.nix の env）。cargo で直接
/// ビルドしたものには無い
const TEXTLINT: Option<&str> = option_env!("CLAUDE_HOOKS_TEXTLINT");
const TEXTLINT_CONFIG: Option<&str> = option_env!("CLAUDE_HOOKS_TEXTLINT_CONFIG");

/// `git merge args…` の本文。`-m` の値（複数なら空行で連ねる）か、`-F` のファイル（dir からの相対）の内容。
/// 本文の無い形は Ok(None)、`-F` のファイルが読めなければ Err にパス
fn merge_message(args: &[Arg], dir: &Path) -> Result<Option<String>, String> {
    let _ = (args, dir);
    Ok(None)
}

/// commit-msg が textlint に渡す部分（modules/home/parts/git.nix の ja_check）。件名が Merge / fixup! /
/// squash! / Revert なら件名を除き、scissors の行から後を除く。空白だけなら None
fn checked_text(message: &str) -> Option<String> {
    let _ = message;
    None
}

/// textlint の結果
#[derive(Debug, PartialEq)]
enum Lint {
    Clean,
    /// 崩れた語がある。「誤り => 正しい語」の行
    Terms(Vec<String>),
    /// textlint を起動できない、または検査に失敗した。理由
    Failed(String),
}

/// commit-msg と同じ引数で textlint を呼ぶ
fn lint(textlint: &str, config: &str, text: &str) -> Lint {
    let _ = (textlint, config, text);
    Lint::Clean
}

impl CheckMergeBodyJaTerms {
    /// `textlint` は埋めた textlint と設定のパス（無ければ判定できない）
    fn check_with(
        &self,
        input: &Input,
        shell: &Shell,
        textlint: Option<(&str, &str)>,
    ) -> Vec<Finding> {
        let _ = (input, shell, textlint);
        Vec::new()
    }
}

impl Rule for CheckMergeBodyJaTerms {
    fn name(&self) -> &'static str {
        "check-merge-body-ja-terms"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        self.check_with(input, shell, TEXTLINT.zip(TEXTLINT_CONFIG))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::take_notices;
    use std::path::PathBuf;
    use std::process::Command;

    /// テストの間だけある一時のディレクトリ（テストが panic しても消す）
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            let dir = std::env::temp_dir().join(format!(
                "claude-hooks-merge-body-{tag}-{}",
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

    /// textlint の代わり。commit-msg と同じ引数で呼ばれたときだけ、本文の「誤語」を prh の形（誤語 => 正語）で
    /// 報告して 1 で、「失敗」を含めば 2 で終わる。辞書の実物の語はコミットの語の検査に掛かるので使わない
    /// （実物の textlint と辞書は modules/home/parts/tests/claude-hooks-merge-ja-terms.sh が確かめる）
    const FAKE_TEXTLINT: &str = r#"#!/bin/sh
[ "$1 $2 $3 $4 $5 $6" = "--config CONFIG --no-color --stdin --stdin-filename COMMIT_EDITMSG.txt" ] || exit 3
body=$(cat)
case "$body" in
  *失敗*) echo "boom" >&2; exit 2 ;;
  *誤語*)
    printf '%s\n' "COMMIT_EDITMSG.txt" "  2:1  ✓ error  誤語 => 正語  prh" "" "✖ 1 problem (1 error, 0 warnings, 0 infos)"
    exit 1 ;;
esac
exit 0
"#;

    fn fake_textlint(dir: &TempDir) -> String {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.0.join("textlint");
        std::fs::write(&path, FAKE_TEXTLINT).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn git_init(dir: &Path) {
        let ok = Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir)
            .status()
            .unwrap()
            .success();
        assert!(ok);
    }

    fn args(src: &str) -> Vec<Arg> {
        let cs = Shell::parse(src).commands().to_vec();
        let (_, args) = cs[0].git_subcommand().unwrap();
        args.to_vec()
    }

    fn message(src: &str) -> Result<Option<String>, String> {
        merge_message(&args(src), Path::new("."))
    }

    fn check_in(command: &str, cwd: &Path, textlint: Option<(&str, &str)>) -> Vec<Finding> {
        let json = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": command},
            "cwd": cwd,
        });
        let input = Input::parse(&json.to_string()).unwrap();
        CheckMergeBodyJaTerms.check_with(&input, &Shell::parse(command), textlint)
    }

    /// git のリポジトリの中で、偽の textlint で検査する
    fn check(command: &str) -> Vec<Finding> {
        let dir = TempDir::new("check");
        git_init(&dir.0);
        let textlint = fake_textlint(&dir);
        check_in(command, &dir.0, Some((&textlint, "CONFIG")))
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
    fn checked_text_drops_merge_subject_like_commit_msg() {
        let t = checked_text("Merge: 誤語\n\n## Why\n本文").unwrap();
        assert!(!t.contains("誤語") && t.contains("本文"), "{t}");
        for subject in ["fixup! 誤語", "squash! 誤語", "Revert \"誤語\""] {
            let t = checked_text(&format!("{subject}\n\n本文")).unwrap();
            assert!(!t.contains("誤語"), "{t}");
        }
    }

    #[test]
    fn checked_text_keeps_other_subjects() {
        assert!(checked_text("feat: 誤語\n\n本文").unwrap().contains("誤語"));
    }

    #[test]
    fn checked_text_skips_comment_lines_before_subject() {
        let t = checked_text("# 注\nMerge: 誤語\n\n本文").unwrap();
        assert!(!t.contains("誤語") && t.contains("# 注"), "{t}");
    }

    #[test]
    fn checked_text_drops_after_scissors() {
        let t = checked_text(&format!(
            "Merge: a\n\n本文\n# {} >8 {}\n誤語",
            "-".repeat(24),
            "-".repeat(24)
        ))
        .unwrap();
        assert!(t.contains("本文") && !t.contains("誤語"), "{t}");
    }

    #[test]
    fn checked_text_is_none_for_blank_body() {
        assert_eq!(checked_text("Merge: a"), None);
        assert_eq!(checked_text("Merge: a\n\n  \n"), None);
    }

    #[test]
    fn lint_classifies_textlint_results() {
        let dir = TempDir::new("lint");
        let t = fake_textlint(&dir);
        assert_eq!(lint(&t, "CONFIG", "確認\n"), Lint::Clean);
        assert_eq!(
            lint(&t, "CONFIG", "誤語\n"),
            Lint::Terms(vec!["誤語 => 正語".to_string()])
        );
        let Lint::Failed(why) = lint(&t, "CONFIG", "失敗\n") else {
            panic!("終了コード 2 は失敗のはず");
        };
        assert!(why.contains("boom"), "{why}");
        assert!(matches!(
            lint("/nonexistent/textlint", "CONFIG", "確認\n"),
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
        let textlint = fake_textlint(&dir);
        std::fs::write(dir.0.join("msg.txt"), "Merge: a\n\n誤語\n").unwrap();
        let cmd = format!(
            "cd {} && git merge --no-ff -F msg.txt feat/a",
            dir.0.display()
        );
        let findings = check_in(&cmd, Path::new("/"), Some((&textlint, "CONFIG")));
        assert!(deny_reason(&findings).contains("誤語 => 正語"));
    }

    #[test]
    fn allows_broken_term_only_in_merge_subject() {
        assert!(check("git merge --no-ff feat/a -m 'Merge: 誤語' -m '本文'").is_empty());
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
        let textlint = fake_textlint(&dir);
        let findings = check_in(
            "git merge -m 'Merge: a' -m '誤語' feat/a",
            &dir.0,
            Some((&textlint, "CONFIG")),
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
        let textlint = fake_textlint(&dir);
        let findings = check_in(
            "git merge -m 'Merge: a' -m '誤語' feat/a",
            &dir.0,
            Some((&textlint, "CONFIG")),
        );
        assert!(findings.is_empty());
    }

    #[test]
    fn reports_undecidable_when_textlint_is_not_embedded() {
        let dir = TempDir::new("none");
        git_init(&dir.0);
        let findings = check_in("git merge -m 'Merge: a' -m '誤語' feat/a", &dir.0, None);
        assert!(findings.is_empty());
        assert!(
            take_notices().iter().any(|n| n.contains("textlint")),
            "判定できなかった理由を出すはず"
        );
    }
}
