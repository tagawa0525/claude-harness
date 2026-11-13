//! check-merge-body-ja-terms: `git merge -m/-F` のメッセージを、実行の前に commit-msg の hook と同じ検査で判定する
//!
//! git の hook は pre-merge-commit（run-checks --merge、数分）→ commit-msg（ja-terms commit-msg）の順に呼ばれる。
//! 本文に辞書（ja-terms の prh.yml）で崩れた語があるか、Merge などで始まらない件名が Conventional Commits の形と
//! 長さに合わないと、数分の品質チェックを待ってから commit-msg が拒む。pre-merge-commit からは `-m` の本文が読めず、
//! git の hook の順も変えられないので、Claude のツール呼び出しの前に走るこのルールで、commit-msg に渡るのと同じ
//! メッセージを検査する（ADR-0050、ADR-0085。ja-terms は [`ja_terms`] が、Linux では package.nix のラッパーが渡すパスで、
//! Windows では PATH で探す）。マージコミットを作るのが
//! 確かな形（[`makes_merge_commit`]）は commit-msg と同じ `ja-terms commit-msg` に、ほかの形（fast-forward になれば
//! commit-msg が呼ばれない、エディタで件名を直せる）は本文の語だけを `ja-terms message` に渡す。
//!
//! 対象は本文を持つ形（`-m` / `--message`、`-F` / `--file`）。本文の無い形（エディタ）は対象外。
//! commit-msg を呼ばない形（`--ff-only`、`--squash`、`--no-commit`、`--no-verify`、グローバルの core.hooksPath を
//! 使わないリポジトリ）と、commit-msg が検査しない所（fork、ローカルの commit-msg に置き換わるリポジトリ）も対象外。
//! 検査を起動できないか、検査に失敗したときは、判定できなかった理由を利用者に見せて通す（ADR-0033。
//! commit-msg が同じ検査で止める）。エスケープは設けない（メッセージを直せば通る）

use super::Rule;
use super::check_commit_message::{Verdict, run_check};
use crate::git;
use crate::git_args::{Item, items, items_with_values};
use crate::input::Input;
use crate::output::{Finding, notice};
use crate::shell::{self, Arg, Cmd, Shell};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub struct CheckMergeBodyJaTerms;

/// ja-terms の store のパスを持つ環境変数。package.nix のラッパーが固定する（cargo で直接ビルドした
/// ものには無く、Windows は PATH の ja-terms を使う。[`ja_terms`]）。git commit のルールも使う
pub(super) const JA_TERMS: &str = "CLAUDE_HOOKS_JA_TERMS";

/// 呼ぶ ja-terms。git merge と git commit のルールと、commit-msg の git hook（crate::git_hook）が使う
pub(crate) fn ja_terms() -> Option<String> {
    crate::tools::find(JA_TERMS, "ja-terms")
}

/// 本文を判定できない理由
#[derive(Debug, PartialEq)]
pub(super) enum Undecided {
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
pub(super) fn merge_message(args: &[Arg], dir: &Path) -> Result<Option<String>, Undecided> {
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
pub(super) fn literal(value: &str, arg: &Arg, option: &'static str) -> Result<String, Undecided> {
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

/// マージコミットを作り、commit-msg が `-m` / `-F` のメッセージをそのまま受けるのが確かな形か: 最後の
/// `--ff` / `--no-ff` / `--ff-only` が `--no-ff` で、`-e` / `--edit`（エディタで直せる）が無い。ほかの形は、
/// fast-forward になれば commit-msg が呼ばれず、エディタで件名を直せるので、件名の形と長さでは止めない
fn makes_merge_commit(args: &[Arg]) -> bool {
    let (mut no_ff, mut edit) = (false, false);
    for item in items("merge", args) {
        match item {
            Item::Long("--no-ff") => no_ff = true,
            Item::Long("--ff" | "--ff-only") => no_ff = false,
            Item::Long("--edit") | Item::Short('e') => edit = true,
            Item::Long("--no-edit") => edit = false,
            _ => {}
        }
    }
    no_ff && !edit
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

/// 本文に残る -F のファイルの名前（書いたまま。`--no-message` はそれまでの -F を本文から外す）
fn body_files(args: &[Arg]) -> Vec<&str> {
    let mut files = Vec::new();
    for (item, value) in items_with_values("merge", args) {
        match (message_option(&item), value) {
            (Some(MessageOption::File), Some((value, _))) => files.push(value),
            (Some(MessageOption::NoMessage), _) => files.clear(),
            _ => {}
        }
    }
    files
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

/// `ja-terms message` の結果
#[derive(Debug, PartialEq)]
enum Lint {
    Clean,
    /// 崩れた語がある。「誤り => 正しい語」の行
    Terms(Vec<String>),
    /// 検査を起動できない、または検査に失敗した。理由
    Failed(String),
}

/// メッセージを標準入力で `ja-terms message` に渡す（commit-msg の hook の `ja-terms commit-msg` と同じ本文の語の
/// 検査）。辞書はビルドに埋めてあるので、実行する場所（コマンドの行き先。commit-msg はリポジトリの直下）は判定に
/// 効かない
fn lint(check: &str, dir: &Path, message: &str) -> Lint {
    let child = Command::new(check)
        .arg("message")
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            return Lint::Failed(format!(
                "{check} を起動できません（{e}。Windows では claude-harness の README の「Windows で使う」の手順で ~/.claude/bin に入れる）"
            ));
        }
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
        // 報告の行（`2:1: 誤り => 正しい語`）から「誤り => 正しい語」を取り出す
        Some(1) => {
            let terms: Vec<String> = stdout
                .lines()
                .filter(|l| l.contains(" => "))
                .map(|l| {
                    l.splitn(3, ':')
                        .nth(2)
                        .map_or(l, str::trim_start)
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

/// 同じコマンドの前段（`writer`）が `-F <path>` のファイルを書きうるときに止める文言。`sub` は git のサブコマンド
/// （commit か merge）。拒むとコマンド全体が止まり、前段も実行されない
pub(super) fn split_calls_reason(sub: &str, path: &str, writer: &str) -> String {
    let (inline, late) = if sub == "commit" {
        (
            "`-F -` のヒアドキュメント（`git commit -F - <<'EOF'` … `EOF`）か `-m \"$(cat <<'EOF' … EOF)\"`",
            "commit-msg の hook が後で拒んでも、同じコマンドの後段は古い HEAD のまま走ります",
        )
    } else {
        (
            "`-m \"$(cat <<'EOF' … EOF)\"`",
            "数分の pre-merge-commit の品質チェックの後で、commit-msg の hook が拒みます",
        )
    };
    format!(
        "git {sub} -F {path} のファイルを同じコマンドの前段（{writer}）が書きうるので、実行の前に本文を検査できません。\
         ファイルを書く呼び出しと git {sub} を別の呼び出しに分けてください（本文を {inline} で git {sub} に直接渡してもかまいません）。\
         拒んだときはコマンド全体が止まるので、前段も実行されていません（ファイルは書く前のままです）。書く前の中身で\
         判定して通すと、書いた後の本文に誤りがあれば、{late}"
    )
}

/// 本文に使う -F のファイル（書いたままの値。シェルが展開する値を除く）のどれかを、同じコマンドの前段が引数に
/// 取るかリダイレクトで書くなら、止める文言。前段の後の中身は実行の前に読めない（#489）
pub(super) fn earlier_writer_reason(
    shell: &Shell,
    cmd: &Cmd,
    base: &Path,
    dir: &Path,
    sub: &str,
    files: &[&str],
) -> Option<String> {
    let home = shell::home();
    files
        .iter()
        .filter(|f| !f.contains(['$', '`']))
        .find_map(|f| {
            let target = shell::resolve(&home, dir, f);
            shell
                .writer_before(cmd, base, &target)
                .map(|w| split_calls_reason(sub, f, &w.name))
        })
}

/// 崩れた語を止める文言
fn terms_reason(terms: &[String]) -> String {
    let list: Vec<String> = terms.iter().map(|t| format!("  {t}")).collect();
    format!(
        "git merge の本文に、崩れた日本語の語があります（ja-terms の辞書）:\n{}\n\
         本文の語を「誤り => 正しい語」のとおりに置き換えてから、もう一度実行してください。このまま実行すると、\
         数分かかる pre-merge-commit の品質チェックの後で、commit-msg の hook が同じ検査で拒みます",
        list.join("\n")
    )
}

/// commit-msg の検査に通らないメッセージを止める文言
fn deny_reason(output: &str) -> String {
    format!(
        "git merge のメッセージが、commit-msg の hook と同じ検査に通りません:\n{output}\n\
         上の ❌ の行のとおりにメッセージを直してから（崩れた語は「誤り => 正しい語」のとおりに置き換えてから）、\
         もう一度実行してください。このまま実行すると、数分かかる pre-merge-commit の品質チェックの後で、\
         commit-msg の hook が同じ検査で拒みます"
    )
}

/// commit-msg が検査しない所か: git のリポジトリの外（コミットできない）、グローバルの core.hooksPath
/// （commit-msg の置き場）を使わないリポジトリ、`upstream` リモートのある fork、ローカルの commit-msg に置き換わる
/// リポジトリ（commit-msg の git hook の crate::git_hook と同じ判定。ローカルの hook は git::local_hook で探す）
pub(super) fn commit_msg_skips(dir: &Path, global_hooks: Option<String>) -> bool {
    // リポジトリの外の `git config --get` はグローバルの値を返すので、置き場の比較より先に見る
    let Some(common) = git::git(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ) else {
        return true;
    };
    if global_hooks.is_none()
        || git::git(dir, &["config", "--get", "core.hooksPath"]) != global_hooks
    {
        return true;
    }
    if git::is_fork(dir) {
        return true;
    }
    git::local_hook(Path::new(&common), "commit-msg").is_some()
}

impl CheckMergeBodyJaTerms {
    /// `check` は ja-terms のパス（無ければ判定できない）。`global_hooks` は dir で見た
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
            if let Some(reason) =
                earlier_writer_reason(shell, cmd, &input.cwd, &dir, "merge", &body_files(args))
            {
                out.push(Finding::Deny(reason));
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
                        "check-merge-body-ja-terms: git merge のメッセージを判定していません（{why}）"
                    ));
                    continue;
                }
            };
            let Some(check) = check else {
                notice(format!(
                    "check-merge-body-ja-terms: ja-terms のパス（環境変数 {JA_TERMS}）が無いので、git merge のメッセージを判定していません（Linux では nix のラッパーが渡す。cargo で直接ビルドしたものは検査しない）"
                ));
                continue;
            };
            if !makes_merge_commit(args) {
                match lint(check, &dir, &message) {
                    Lint::Clean => {}
                    Lint::Terms(terms) => out.push(Finding::Deny(terms_reason(&terms))),
                    Lint::Failed(why) => notice(format!(
                        "check-merge-body-ja-terms: git merge の本文の語を判定していません: {why}"
                    )),
                }
                continue;
            }
            match run_check(check, &dir, &message) {
                Verdict::Pass => {}
                Verdict::Reject(output) => out.push(Finding::Deny(deny_reason(&output))),
                Verdict::Failed(why) => notice(format!(
                    "check-merge-body-ja-terms: git merge のメッセージを判定していません: {why}"
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
        let check = ja_terms();
        self.check_with(input, shell, check.as_deref(), &|dir| {
            git::git(dir, &["config", "--global", "--get", "core.hooksPath"])
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::{NOTICES_LOCK as NOTICES, take_notices};
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

    /// ja-terms の代わり。`commit-msg <ファイル>` では、メッセージに「形違い」があれば commit-msg の形の件名の ❌ を、
    /// 「誤語」があれば報告の行（行:列: 誤語 => 正語）と ❌ を出して 1 で終わる。`message` では、標準入力の「誤語」を
    /// 報告の行で出して 1 で終わる（形は見ない）。どちらも「無語」なら何も出さずに 1 で、「失敗」を含めば 2 で終わる。辞書の実物の語はコミットの語の検査に掛かるので使わない
    /// （実物の ja-terms と辞書は tests/merge-ja-terms.sh が確かめる）
    #[cfg(unix)]
    const FAKE_CHECK: &str = r#"#!/bin/sh
if [ $# -eq 2 ] && [ "$1" = commit-msg ]; then
  body=$(cat "$2")
  case "$body" in
    *失敗*) echo "boom" >&2; exit 2 ;;
    *無語*) exit 1 ;;
    *形違い*)
      echo "❌ Conventional Commits 形式ではありません: 形違い"
      exit 1 ;;
    *誤語*)
      printf '%s\n' "2:1: 誤語 => 正語" "❌ コミットの本文に崩れた日本語の語があります。"
      exit 1 ;;
  esac
  exit 0
fi
[ $# -eq 1 ] && [ "$1" = message ] || exit 3
body=$(cat)
case "$body" in
  *失敗*) echo "boom" >&2; exit 2 ;;
  *無語*) exit 1 ;;
  *誤語*)
    printf '%s\n' "2:1: 誤語 => 正語"
    exit 1 ;;
esac
exit 0
"#;

    /// 偽の ja-terms を置き、そのパスを返す。Windows はシバンの sh のスクリプトを実行ファイルとして起動できないので、sh の偽物を使うテストは Unix だけで流す（ADR-0083）
    #[cfg(unix)]
    fn fake_check(dir: &TempDir) -> String {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.0.join("ja-terms");
        std::fs::write(&path, FAKE_CHECK).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        // 並行するテストが子を fork した直後は、書き込みに開いた fd が exec までその子に残り、実行が
        // ETXTBSY（Text file busy）で失敗する。引数つき（偽の検査は 3 で終わる）で起動できるまで待つ
        for _ in 0..100 {
            match Command::new(&path).arg("probe").output() {
                Err(e) if e.raw_os_error() == Some(26) => {
                    std::thread::sleep(std::time::Duration::from_millis(10))
                }
                _ => return path.to_string_lossy().into_owned(),
            }
        }
        panic!(
            "偽の検査が 1 秒たっても Text file busy のまま: {}",
            path.display()
        );
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

    /// git のリポジトリの中で、偽の ja-terms で検査する
    #[cfg(unix)]
    fn check(command: &str) -> Vec<Finding> {
        let dir = TempDir::new("check");
        git_init(&dir.0);
        let check = fake_check(&dir);
        check_in(command, &dir.0, Some(&check))
    }

    fn only_deny(findings: &[Finding]) -> String {
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
    fn message_keeps_lines_of_double_quoted_m_option() {
        assert_eq!(
            message("git merge --no-ff feat/a -m \"Merge: a\n\n## Why\n本文の語\n\""),
            Ok(Some("Merge: a\n\n## Why\n本文の語\n".to_string()))
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

    #[cfg(unix)]
    #[test]
    fn a_failed_or_missing_check_is_undecided() {
        let _lock = NOTICES.lock().unwrap_or_else(|e| e.into_inner());
        take_notices();
        assert!(check("git merge --no-ff feat/a -m 'Merge: a' -m '失敗'").is_empty());
        assert!(take_notices().iter().any(|n| n.contains("boom")));
        let dir = TempDir::new("missing");
        git_init(&dir.0);
        let findings = check_in(
            "git merge --no-ff feat/a -m 'Merge: a' -m '確認'",
            &dir.0,
            Some("/nonexistent/ja-terms"),
        );
        assert!(findings.is_empty());
        assert!(!take_notices().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_body_check_is_undecided() {
        // --no-ff の無い形（本文の語だけを見る ja-terms message）の失敗も、止めずに理由を出す
        let _lock = NOTICES.lock().unwrap_or_else(|e| e.into_inner());
        take_notices();
        assert!(check("git merge feat/a -m 'Merge: a' -m '失敗'").is_empty());
        assert!(take_notices().iter().any(|n| n.contains("boom")));
    }

    #[cfg(unix)]
    #[test]
    fn denies_a_subject_commit_msg_rejects() {
        // commit-msg は Merge で始まらない件名の形と長さも見るので、同じ検査で先に止める
        let r = only_deny(&check("git merge --no-ff feat/a -m '形違い'"));
        assert!(r.contains("Conventional Commits"), "{r}");
        assert!(r.contains("pre-merge-commit"), "{r}");
    }

    #[cfg(unix)]
    #[test]
    fn denies_broken_term_with_correction_and_reason() {
        let r = only_deny(&check(
            "git merge --no-ff feat/a -m 'Merge: a' -m '誤語する'",
        ));
        assert!(r.contains("誤語 => 正語"), "{r}");
        // 理由（数分後に commit-msg で拒まれる）と修正方法（語の置き換え）
        assert!(r.contains("commit-msg"), "{r}");
        assert!(r.contains("置き換え"), "{r}");
    }

    #[cfg(unix)]
    #[test]
    fn denies_broken_term_in_heredoc_message() {
        let r = only_deny(&check(
            "git merge --no-ff feat/a -m \"$(cat <<'EOF'\nMerge: a\n\n## Why\n誤語\nEOF\n)\"",
        ));
        assert!(r.contains("誤語 => 正語"), "{r}");
    }

    #[cfg(unix)]
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
        assert!(only_deny(&findings).contains("誤語 => 正語"));
    }

    /// 同じコマンドで -F のファイルを書き換えてから merge する形と、merge だけの形
    #[cfg(unix)]
    fn deny_for(prefix: &str) -> String {
        let dir = TempDir::new("sep");
        git_init(&dir.0);
        let check = fake_check(&dir);
        std::fs::write(dir.0.join("msg.txt"), "Merge: a\n\n誤語\n").unwrap();
        let cmd = format!(
            "cd {} && {prefix}git merge --no-ff -F msg.txt feat/a",
            dir.0.display()
        );
        only_deny(&check_in(&cmd, Path::new("/"), Some(&check)))
    }

    #[cfg(unix)]
    #[test]
    fn denies_message_file_written_earlier_without_reading_it() {
        // 前段が書いた後の中身は実行の前に読めないので、書く前の中身（ここでは崩れた語）では判定しない
        for prefix in [
            "sed -i 's/誤語/正語/' msg.txt && ",
            "cat > ./msg.txt <<'EOF'\nMerge: a\n\n正語\nEOF\n",
            // 書き先のパスを、同じ呼び出しで字面のまま決まる変数で書く
            "M=msg; cat > $M.txt <<'EOF'\nMerge: a\n\n正語\nEOF\n",
            "M=msg.txt; sed -i 's/誤語/正語/' \"${M}\" && ",
        ] {
            let r = deny_for(prefix);
            assert!(r.contains("前段も実行されていません"), "{prefix}: {r}");
            assert!(r.contains("分けて"), "{prefix}: {r}");
            assert!(r.contains("-m \"$(cat <<'EOF'"), "{prefix}: {r}");
            assert!(!r.contains("誤語 => 正語"), "{prefix}: {r}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn denies_lone_file_merge_without_split_calls_hint() {
        let r = deny_for("");
        assert!(r.contains("誤語 => 正語"), "{r}");
        assert!(!r.contains("分けてください"), "{r}");
    }

    #[cfg(unix)]
    #[test]
    fn denies_unrelated_chained_command_without_split_calls_hint() {
        let r = deny_for("git switch main && ");
        assert!(r.contains("誤語 => 正語"), "{r}");
        assert!(!r.contains("分けてください"), "{r}");
    }

    #[cfg(unix)]
    #[test]
    fn denies_wrapped_merge_without_split_calls_hint() {
        let r = deny_for("true && bash -c 'git status' && ");
        assert!(!r.contains("分けてください"), "{r}");
    }

    #[cfg(unix)]
    #[test]
    fn denies_inline_message_without_split_calls_hint() {
        let r = only_deny(&check(
            "true && git merge --no-ff feat/a -m 'Merge: a' -m '誤語する'",
        ));
        assert!(!r.contains("分けてください"), "{r}");
    }

    #[test]
    fn body_files_drops_files_before_no_message() {
        assert_eq!(
            body_files(&args("git merge -F msg.txt feat/a")),
            ["msg.txt"]
        );
        assert!(
            body_files(&args(
                "git merge -F msg.txt --no-message -m 'Merge: a' feat/a"
            ))
            .is_empty()
        );
    }

    #[cfg(unix)]
    #[test]
    fn allows_clean_message() {
        assert!(check("git merge --no-ff feat/a -m 'Merge: a' -m '確認'").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn ignores_merge_without_message() {
        assert!(check("git merge --no-ff feat/a").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn ignores_other_commands_with_broken_terms() {
        assert!(check("echo 誤語 && git commit -m 'feat: 誤語'").is_empty());
    }

    #[cfg(unix)]
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

    #[cfg(unix)]
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
    fn local_commit_msg_hook_counts_by_the_exec_bit_on_unix_and_by_the_file_on_windows() {
        // 実行の bit を付けない hook のファイル。Git for Windows は実行し、Unix の git は実行しない
        let dir = TempDir::new("plain-hook");
        git_init(&dir.0);
        std::fs::write(dir.0.join(".git/hooks/commit-msg"), "#!/bin/sh\nexit 0\n").unwrap();
        assert_eq!(
            super::commit_msg_skips(&dir.0, global_hooks(&dir.0)),
            !cfg!(unix)
        );
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
                .any(|n| n.contains("CLAUDE_HOOKS_JA_TERMS")),
            "判定できなかった理由を出すはず"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_rejection_without_output_is_undecided() {
        // 何を直すかを示せないので、止めずに判定できなかった理由を出す（commit-msg が同じ検査で止める）
        let _lock = NOTICES.lock().unwrap_or_else(|e| e.into_inner());
        take_notices();
        for cmd in [
            "git merge --no-ff feat/a -m 'Merge: a' -m '無語'",
            "git merge feat/a -m 'Merge: a' -m '無語'",
        ] {
            assert!(check(cmd).is_empty(), "{cmd}");
            assert!(!take_notices().is_empty(), "{cmd}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn checks_only_the_body_terms_when_git_may_not_make_a_merge_commit() {
        // --no-ff の無い merge は fast-forward になれば commit-msg を呼ばず、-e はエディタで件名を直せるので、
        // 件名の形では止めず、本文の語だけを見る
        for cmd in [
            "git merge feat/a -m '形違い'",
            "git merge --no-ff --ff feat/a -m '形違い'",
            "git merge --no-ff -e feat/a -m '形違い'",
        ] {
            assert!(check(cmd).is_empty(), "{cmd}");
        }
        let r = only_deny(&check("git merge feat/a -m '形違い' -m '誤語'"));
        // 報告の行の「行:列:」を外した語の組を示す
        assert!(r.lines().any(|l| l == "  誤語 => 正語"), "{r}");
    }

    #[cfg(unix)]
    #[test]
    fn ignores_merge_that_makes_no_commit() {
        assert!(check("git merge --ff-only feat/a -m 'Merge: a' -m '誤語'").is_empty());
        assert!(check("git merge --squash feat/a -m 'Merge: a' -m '誤語'").is_empty());
    }

    #[cfg(unix)]
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

    #[cfg(unix)]
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

    #[cfg(unix)]
    #[test]
    fn checks_merge_whose_last_option_makes_a_commit() {
        let r = only_deny(&check(
            "git merge --ff-only --no-ff feat/a -m 'Merge: a' -m '誤語'",
        ));
        assert!(r.contains("誤語 => 正語"), "{r}");
        let r = only_deny(&check(
            "git merge --squash --no-squash feat/a -m 'Merge: a' -m '誤語'",
        ));
        assert!(r.contains("誤語 => 正語"), "{r}");
    }

    #[cfg(unix)]
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

    #[cfg(unix)]
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

    #[cfg(unix)]
    #[test]
    fn checks_literal_dollar_in_single_quotes() {
        let r = only_deny(&check(
            "git merge --no-ff feat/a -m 'Merge: a' -m '$x 誤語'",
        ));
        assert!(r.contains("誤語 => 正語"), "{r}");
    }

    #[cfg(unix)]
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

    #[cfg(unix)]
    #[test]
    fn reports_quoted_text_joined_with_expansion_as_undecidable() {
        let _lock = NOTICES.lock().unwrap_or_else(|e| e.into_inner());
        assert!(check("git merge --no-ff feat/a -m 'Merge: a' -m '誤語 '\"$X\"").is_empty());
        assert!(
            take_notices().iter().any(|n| n.contains("展開")),
            "判定できなかった理由を出すはず"
        );
    }

    #[cfg(unix)]
    #[test]
    fn ignores_merge_with_no_commit() {
        assert!(check("git merge --no-ff --no-commit feat/a -m 'Merge: a' -m '誤語'").is_empty());
        let r = only_deny(&check(
            "git merge --no-ff --no-commit --commit feat/a -m 'Merge: a' -m '誤語'",
        ));
        assert!(r.contains("誤語 => 正語"), "{r}");
    }

    #[cfg(unix)]
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

    #[cfg(unix)]
    #[test]
    fn checks_attached_single_quoted_value_with_dollar() {
        for src in [
            "git merge --no-ff feat/a -m'Merge: a' -m'費用 $5 の誤語'",
            "git merge --no-ff feat/a -m 'Merge: a' --message='費用 $5 の誤語'",
        ] {
            let r = only_deny(&check(src));
            assert!(r.contains("誤語 => 正語"), "{src}: {r}");
        }
    }
}
