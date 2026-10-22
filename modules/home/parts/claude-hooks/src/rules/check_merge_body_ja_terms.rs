//! check-merge-body-ja-terms: `git merge -m/-F` の本文の崩れた日本語の語を、実行の前に検査する
//!
//! git の hook は pre-merge-commit（run-checks --merge、数分）→ commit-msg（textlint、数秒）の順に呼ばれる。
//! 本文に辞書（ja-terms の prh.yml）で崩れた語があると、数分の品質チェックを待ってから commit-msg が拒む。
//! pre-merge-commit からは `-m` の本文が読めず、git の hook の順も変えられないので、Claude のツール呼び出しの
//! 前に走るこのルールで、commit-msg が呼ぶのと同じ検査（ja-terms-message。modules/home/parts/ja-terms/
//! message.sh。検査する範囲と textlint の設定を持つ。store のパスは package.nix が包んで渡す）に本文を渡す（ADR-0050）。
//!
//! 対象は本文を持つ形（`-m` / `--message`、`-F` / `--file`）。本文の無い形（エディタ）は対象外。
//! commit-msg を呼ばない形（`--ff-only`、`--squash`、`--no-commit`、`--no-verify`、グローバルの core.hooksPath を
//! 使わないリポジトリ）と、commit-msg が検査しない所（fork、ローカルの commit-msg に置き換わるリポジトリ）も対象外。
//! 検査を起動できないか、検査に失敗したときは、判定できなかった理由を利用者に見せて通す（ADR-0033。
//! commit-msg が同じ検査で止める）。エスケープは設けない（語を直せば通る）

use super::Rule;
use crate::git;
use crate::git_args::{Item, items, items_with_values};
use crate::input::Input;
use crate::output::{Finding, notice};
use crate::shell::{self, Arg, Shell};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub struct CheckMergeBodyJaTerms;

/// ja-terms-message の store のパスを持つ環境変数。package.nix の包む側が固定する（cargo で直接ビルドした
/// ものには無い）
const JA_TERMS_MESSAGE: &str = "CLAUDE_HOOKS_JA_TERMS_MESSAGE";

/// 本文を判定できない理由
#[derive(Debug, PartialEq)]
enum Undecided {
    /// シェルが展開する値（`-m "$MSG"`、`-F "$TMPDIR/m"`）。オプションの名前
    Expanded(&'static str),
    /// 読めない `-F` のファイル
    Unreadable(String),
}

/// 本文を決めるオプション
#[derive(Clone, Copy)]
enum MessageOption {
    Message,
    File,
    NoMessage,
}

fn message_option(item: &Item) -> Option<MessageOption> {
    match item {
        Item::Short('m') | Item::Long("--message") => Some(MessageOption::Message),
        Item::Short('F') | Item::Long("--file") => Some(MessageOption::File),
        Item::Long("--no-message") => Some(MessageOption::NoMessage),
        _ => None,
    }
}

/// `git merge args…` の本文を、git と同じ順で組み立てる（builtin/merge.c の option_parse_message と
/// option_read_message）: `-m` の前は空行、`-F` の前は改行 1 つ（どちらも先に本文があるときだけ）、
/// `--no-message` はそれまでを消す。`-F` は `-` も名前のファイルとして、dir からの相対で読む（`~` は HOME）。
/// git は先頭の空行や --cleanup の整えを commit-msg の後で行うので、書いたままにする。本文の無い形は Ok(None)
fn merge_message(args: &[Arg], dir: &Path) -> Result<Option<String>, Undecided> {
    let mut buf = String::new();
    let mut have = false;
    for (item, value) in items_with_values("merge", args) {
        match (message_option(&item), value) {
            (Some(MessageOption::Message), Some((value, arg))) => {
                if !buf.is_empty() {
                    buf.push_str("\n\n");
                }
                buf.push_str(&literal(value, arg, "-m")?);
                have = true;
            }
            (Some(MessageOption::File), Some((value, arg))) => {
                let path = literal(value, arg, "-F")?;
                let text = std::fs::read_to_string(shell::resolve(&shell::home(), dir, &path))
                    .map_err(|_| Undecided::Unreadable(path))?;
                if !buf.is_empty() {
                    buf.push('\n');
                }
                buf.push_str(&text);
                have = true;
            }
            (Some(MessageOption::NoMessage), _) => {
                buf.clear();
                have = false;
            }
            _ => {}
        }
    }
    Ok(have.then_some(buf))
}

/// 値を書いたままの文字列にする。ヒアドキュメントを `$(cat <<…)` で渡した形（`-m "$(cat <<'EOF' … EOF)"`）は
/// 本文を取り出す。単引用符で書いた値と、区切りをクォートしたヒアドキュメントは展開されない。ほかで `$` や
/// バッククォートを含むもの（単引用符に `"$X"` が続く値を含む）は、シェルが展開した後の値が分からない
fn literal(value: &str, arg: &Arg, option: &'static str) -> Result<String, Undecided> {
    let expands = |t: &str| t.contains('$') || t.contains('`');
    // 値の全体を単引用符で書いた形（`'v'`、`-m'v'`、`--message='v'`）。語の中の単引用符がその 2 つだけなら、
    // 値の前（オプションの名前）にも後ろにも展開は無い
    let raw = arg.raw.as_str();
    if raw.ends_with(&format!("'{value}'")) && raw.matches('\'').count() == 2 {
        return Ok(value.to_string());
    }
    if let Some((body, quoted)) = heredoc_body(value) {
        return if quoted || !expands(&body) {
            Ok(body)
        } else {
            Err(Undecided::Expanded(option))
        };
    }
    if expands(value) {
        return Err(Undecided::Expanded(option));
    }
    Ok(value.to_string())
}

/// commit-msg を呼ばない指定か（マージコミットを作らない、作る前に止まる、hook を飛ばす）。`--ff` / `--no-ff` /
/// `--ff-only`、`--squash` / `--no-squash`、`--commit` / `--no-commit`、`--verify` / `--no-verify` は、git と同じく
/// 後ろのものが勝つ
fn makes_no_commit(args: &[Arg]) -> bool {
    let (mut ff_only, mut squash, mut no_commit, mut no_verify) = (false, false, false, false);
    for item in items("merge", args) {
        match item {
            Item::Long("--ff-only") => ff_only = true,
            Item::Long("--ff" | "--no-ff") => ff_only = false,
            Item::Long("--squash") => squash = true,
            Item::Long("--no-squash") => squash = false,
            Item::Long("--no-commit") => no_commit = true,
            Item::Long("--commit") => no_commit = false,
            Item::Long("--no-verify") => no_verify = true,
            Item::Long("--verify") => no_verify = false,
            _ => {}
        }
    }
    ff_only || squash || no_commit || no_verify
}

/// `-m` か `-F` があるか
fn has_message(args: &[Arg]) -> bool {
    items_with_values("merge", args).iter().any(|(item, _)| {
        matches!(
            message_option(item),
            Some(MessageOption::Message | MessageOption::File)
        )
    })
}

/// `$(cat <<[-]DELIM\n本文\nDELIM\n)` の本文と、区切りをクォートしたか（'EOF'、"EOF"。本文を展開しない）。
/// `<<-` は行の先頭のタブを外す
fn heredoc_body(text: &str) -> Option<(String, bool)> {
    let rest = text.strip_prefix("$(cat <<")?.strip_suffix(')')?;
    let (dash, rest) = match rest.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, rest),
    };
    let (head, body) = rest.split_once('\n')?;
    let head = head.trim();
    let delim = head.trim_matches(|c| c == '\'' || c == '"');
    let quoted = delim.len() != head.len();
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
            return Some((lines.join("\n"), quoted));
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
        Some(1) => {
            let terms: Vec<String> = stdout
                .lines()
                .filter(|l| l.contains(" => "))
                .map(|l| {
                    let l = l.trim_end().trim_end_matches("prh").trim();
                    l.split_once("error")
                        .map_or(l, |(_, rest)| rest)
                        .trim()
                        .to_string()
                })
                .collect();
            if terms.is_empty() {
                Lint::Failed(format!(
                    "{check} が「誤り => 正しい語」を出さずに 1 で終わりました: {}",
                    stdout.trim()
                ))
            } else {
                Lint::Terms(terms)
            }
        }
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

/// commit-msg が語を検査しないリポジトリか: グローバルの core.hooksPath（commit-msg の置き場）を使わない
/// リポジトリと、`upstream` リモートのある fork、ローカルの commit-msg に置き換わるリポジトリ
/// （modules/home/parts/git.nix の commit-msg と同じ判定）
fn commit_msg_skips(dir: &Path, global_hooks: Option<String>) -> bool {
    if global_hooks.is_none()
        || git::git(dir, &["config", "--get", "core.hooksPath"]) != global_hooks
    {
        return true;
    }
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
    /// `check` は ja-terms-message のパス（無ければ判定できない）。`global_hooks` は dir で見た
    /// グローバルの core.hooksPath
    fn check_with(
        &self,
        input: &Input,
        shell: &Shell,
        check: Option<&str>,
        global_hooks: &dyn Fn(&Path) -> Option<String>,
    ) -> Vec<Finding> {
        let mut out = Vec::new();
        for cmd in shell.commands() {
            let Some(("merge", args)) = cmd.git_subcommand() else {
                continue;
            };
            // 本文の無い形（エディタ）と、commit-msg を呼ばない形は対象外
            if !has_message(args) || makes_no_commit(args) {
                continue;
            }
            let dir = shell.target_dir(cmd, &input.cwd);
            if commit_msg_skips(&dir, global_hooks(&dir)) {
                continue;
            }
            let message = match merge_message(args, &dir) {
                Ok(Some(m)) => m,
                Ok(None) => continue,
                Err(why) => {
                    let why = match why {
                        Undecided::Expanded(opt) => format!("{opt} の値はシェルが展開する"),
                        Undecided::Unreadable(p) => format!("-F {p} を読めない"),
                    };
                    notice(format!(
                        "check-merge-body-ja-terms: git merge の本文の語を判定していません（{why}）"
                    ));
                    continue;
                }
            };
            let Some(check) = check else {
                notice(
                    "check-merge-body-ja-terms: ja-terms-message のパス（環境変数 CLAUDE_HOOKS_JA_TERMS_MESSAGE）が無いので、git merge の本文の語を判定していません（nix で包んだものだけが検査する）".to_string(),
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
        let check = std::env::var(JA_TERMS_MESSAGE).ok();
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
        // 並行するテストが子を fork した直後は、書き込みに開いた fd が exec までその子に残り、実行が
        // ETXTBSY（Text file busy）で失敗する。引数つき（偽の検査は 3 で終わる）で起動できるまで待つ
        for _ in 0..100 {
            match Command::new(&path).arg("probe").output() {
                Err(e) if e.raw_os_error() == Some(26) => {
                    std::thread::sleep(std::time::Duration::from_millis(10))
                }
                _ => break,
            }
        }
        path.to_string_lossy().into_owned()
    }

    /// 判定できない理由（output::notice）はプロセスに 1 つの置き場に集まり、take_notices は全部を取り出す。
    /// 理由を確かめるテストは、互いの理由を取り合わないように順に実行する
    static NOTICES: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

    fn message(src: &str) -> Result<Option<String>, Undecided> {
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
            Err(Undecided::Unreadable("/nonexistent/msg.txt".to_string()))
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
        let _lock = NOTICES.lock().unwrap_or_else(|e| e.into_inner());
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

    #[test]
    fn checks_merge_whose_last_option_makes_a_commit() {
        let r = deny_reason(&check(
            "git merge --ff-only --no-ff feat/a -m 'Merge: a' -m '誤語'",
        ));
        assert!(r.contains("誤語 => 正語"), "{r}");
        let r = deny_reason(&check(
            "git merge --squash --no-squash feat/a -m 'Merge: a' -m '誤語'",
        ));
        assert!(r.contains("誤語 => 正語"), "{r}");
    }

    #[test]
    fn skipped_repository_gets_no_notice() {
        let _lock = NOTICES.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new("skip-notice");
        git_init(&dir.0);
        git(&dir.0, &["config", "core.hooksPath", ".githooks"]);
        let check = fake_check(&dir);
        let findings = check_in(
            "git merge --no-ff -F /nonexistent/skip-notice.txt feat/a",
            &dir.0,
            Some(&check),
        );
        assert!(findings.is_empty());
        assert!(
            !take_notices().iter().any(|n| n.contains("skip-notice.txt")),
            "検査しないリポジトリでは判定できない理由を出さないはず"
        );
    }

    #[test]
    fn message_is_passed_as_written_like_git_to_commit_msg() {
        // git は先頭の空行と --cleanup=strip の # の行を、commit-msg の後で除く
        let dir = TempDir::new("raw");
        std::fs::write(dir.0.join("msg.txt"), "\n\nMerge: a\n\n# c\n本文\n").unwrap();
        assert_eq!(
            merge_message(&args("git merge --cleanup=strip -F msg.txt feat/a"), &dir.0),
            Ok(Some("\n\nMerge: a\n\n# c\n本文\n".to_string()))
        );
    }

    #[test]
    fn reports_message_with_shell_expansion_as_undecidable() {
        let _lock = NOTICES.lock().unwrap_or_else(|e| e.into_inner());
        assert!(check("git merge --no-ff feat/a -m 'Merge: a' -m \"$MSG\"").is_empty());
        assert!(check("git merge --no-ff feat/a -F \"$TMPDIR/msg.txt\"").is_empty());
        let notices = take_notices();
        assert!(
            notices.iter().filter(|n| n.contains("展開")).count() >= 2,
            "{notices:?}"
        );
    }

    #[test]
    fn checks_literal_dollar_in_single_quotes() {
        let r = deny_reason(&check(
            "git merge --no-ff feat/a -m 'Merge: a' -m '$x 誤語'",
        ));
        assert!(r.contains("誤語 => 正語"), "{r}");
    }

    #[test]
    fn ignores_merge_without_hooks() {
        assert!(check("git merge --no-verify --no-ff feat/a -m 'Merge: a' -m '誤語'").is_empty());
    }

    #[test]
    fn message_file_dash_is_a_file_like_git() {
        // git merge の -F は `-` を標準入力でなく、その名前のファイルとして読む（git 2.56 で確かめた）
        let dir = TempDir::new("dash");
        std::fs::write(dir.0.join("-"), "Merge: a\n\n本文\n").unwrap();
        assert_eq!(
            merge_message(&args("git merge --no-ff -F - feat/a"), &dir.0),
            Ok(Some("Merge: a\n\n本文\n".to_string()))
        );
    }

    #[test]
    fn message_is_built_like_git() {
        // -m どうしは空行で、-F の前は改行 1 つで連ね、--no-message はそれまでを消す
        let dir = TempDir::new("build");
        std::fs::write(dir.0.join("f.txt"), "FILE\n").unwrap();
        let build = |src: &str| merge_message(&args(src), &dir.0);
        assert_eq!(
            build("git merge -m A -F f.txt feat/a"),
            Ok(Some("A\nFILE\n".to_string()))
        );
        assert_eq!(
            build("git merge -F f.txt -m B feat/a"),
            Ok(Some("FILE\n\n\nB".to_string()))
        );
        assert_eq!(
            build("git merge -m '誤語' --no-message -m 'Merge: a' feat/a"),
            Ok(Some("Merge: a".to_string()))
        );
    }

    #[test]
    fn reports_quoted_text_joined_with_expansion_as_undecidable() {
        let _lock = NOTICES.lock().unwrap_or_else(|e| e.into_inner());
        assert!(check("git merge --no-ff feat/a -m 'Merge: a' -m '誤語 '\"$X\"").is_empty());
        assert!(
            take_notices().iter().any(|n| n.contains("展開")),
            "判定できなかった理由を出すはず"
        );
    }

    #[test]
    fn ignores_merge_with_no_commit() {
        assert!(check("git merge --no-ff --no-commit feat/a -m 'Merge: a' -m '誤語'").is_empty());
        let r = deny_reason(&check(
            "git merge --no-ff --no-commit --commit feat/a -m 'Merge: a' -m '誤語'",
        ));
        assert!(r.contains("誤語 => 正語"), "{r}");
    }

    #[test]
    fn expansion_notice_names_the_option_not_the_body() {
        let _lock = NOTICES.lock().unwrap_or_else(|e| e.into_inner());
        let long = "x".repeat(200);
        assert!(check(&format!("git merge --no-ff feat/a -m \"$X{long}\"")).is_empty());
        let notices = take_notices();
        assert!(
            notices
                .iter()
                .any(|n| n.contains("展開") && !n.contains(&long)),
            "{notices:?}"
        );
    }

    #[test]
    fn checks_attached_single_quoted_value_with_dollar() {
        for src in [
            "git merge --no-ff feat/a -m'Merge: a' -m'費用 $5 の誤語'",
            "git merge --no-ff feat/a -m 'Merge: a' --message='費用 $5 の誤語'",
        ] {
            let r = deny_reason(&check(src));
            assert!(r.contains("誤語 => 正語"), "{src}: {r}");
        }
    }
}
