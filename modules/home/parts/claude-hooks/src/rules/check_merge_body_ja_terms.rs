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
use crate::git;
use crate::git_args::{Item, option_values};
use crate::input::Input;
use crate::output::{Finding, notice};
use crate::shell::{Arg, Shell};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub struct CheckMergeBodyJaTerms;

/// ビルド時に埋める textlint の実行ファイルと設定の store のパス（package.nix の env）。cargo で直接
/// ビルドしたものには無い
const TEXTLINT: Option<&str> = option_env!("CLAUDE_HOOKS_TEXTLINT");
const TEXTLINT_CONFIG: Option<&str> = option_env!("CLAUDE_HOOKS_TEXTLINT_CONFIG");

/// `git merge args…` の本文。`-m` の値（複数なら空行で連ねる）か、`-F` のファイル（dir からの相対）の内容。
/// 本文の無い形は Ok(None)、`-F` のファイルが読めなければ Err にパス
fn merge_message(args: &[Arg], dir: &Path) -> Result<Option<String>, String> {
    let mut parts = Vec::new();
    for (opt, value) in option_values("merge", args) {
        match opt {
            Item::Short('m') | Item::Long("--message") => parts.push(message_value(value)),
            Item::Short('F') | Item::Long("--file") => {
                match std::fs::read_to_string(dir.join(value)) {
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

/// commit-msg が textlint に渡す部分（modules/home/parts/git.nix の ja_check）。件名が Merge / fixup! /
/// squash! / Revert なら件名を除き、scissors の行から後を除く。空白だけなら None
fn checked_text(message: &str) -> Option<String> {
    let lines: Vec<&str> = message.lines().collect();
    // 件名は # で始まらない最初の行（commit-msg の `sed -n '/^#/!{p;q}'`）。除くのは、空でも # でもない
    // 最初の行（`sed '0,/^[^#]/{//d}'`）
    let subject = lines.iter().find(|l| !l.starts_with('#')).copied();
    let drop_subject = subject.is_some_and(|s| {
        ["Merge", "fixup!", "squash!", "Revert"]
            .iter()
            .any(|p| s.starts_with(p))
    });
    let scissors = format!("# {} >8 {}", "-".repeat(24), "-".repeat(24));
    let mut dropped = !drop_subject;
    let mut out: Vec<&str> = Vec::new();
    for line in lines {
        if line == scissors {
            break;
        }
        if !dropped && !line.is_empty() && !line.starts_with('#') {
            dropped = true;
            continue;
        }
        out.push(line);
    }
    let text = out.join("\n");
    (!text.trim().is_empty()).then(|| text + "\n")
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
    let child = Command::new(textlint)
        .args([
            "--config",
            config,
            "--no-color",
            "--stdin",
            "--stdin-filename",
            "COMMIT_EDITMSG.txt",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return Lint::Failed(format!("{textlint} を起動できません（{e}）")),
    };
    // 本文は数 KB なので、書き終えてから出力を読んでもパイプは詰まらない
    if let Some(mut stdin) = child.stdin.take()
        && let Err(e) = stdin.write_all(text.as_bytes())
    {
        let _ = child.kill();
        return Lint::Failed(format!("textlint に本文を渡せません（{e}）"));
    }
    let out = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => return Lint::Failed(format!("textlint の終わりを待てません（{e}）")),
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
            "textlint が失敗しました（{}）: {}{}",
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
    /// `textlint` は埋めた textlint と設定のパス（無ければ判定できない）
    fn check_with(
        &self,
        input: &Input,
        shell: &Shell,
        textlint: Option<(&str, &str)>,
    ) -> Vec<Finding> {
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
            let Some(text) = checked_text(&message) else {
                continue;
            };
            if commit_msg_skips(&dir) {
                continue;
            }
            let Some((textlint, config)) = textlint else {
                notice(
                    "check-merge-body-ja-terms: このビルドには textlint が埋め込まれていないので、git merge の本文の語を判定していません（nix でビルドしたものだけが検査する）".to_string(),
                );
                continue;
            };
            match lint(textlint, config, &text) {
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
        self.check_with(input, shell, TEXTLINT.zip(TEXTLINT_CONFIG))
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

    /// textlint の代わり。commit-msg と同じ引数で呼ばれたときだけ、本文の「誤語」を prh の形（誤語 => 正語）で
    /// 報告して 1 で、「失敗」を含めば 2 で終わる。辞書の実物の語はコミットの語の検査に掛かるので使わない
    /// （実物の textlint と辞書は tests/merge-ja-terms.sh が確かめる）
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
