//! Bash ツールのコマンド文字列の構文解析（tree-sitter-bash）
//!
//! 旧 bash hook が正規表現とヒアドキュメントのマスクで近似していたことを、AST で決定的に行う:
//! - `heredoc_body` はデータなので配下のコマンドを探さない（本文に書かれたコマンド例に反応しない）
//! - クォート・コマンド置換はノード種別で分かる（`"a << b"` はヒアドキュメントではない、
//!   `-m "git add -A"` はコマンドではない）
//! - `git -C <dir>` や先行する `cd <dir>` は AST 上の位置で判定する
//!
//! コマンドは解析のときに一度だけ読み、規則はそれを共有する:
//! - コマンドを包むもの（env、command、exec、nohup、time、nice、timeout、setsid、stdbuf、ionice）はほどき、
//!   中のコマンドと、前置きの代入や包みが変える環境変数・ディレクトリを `Cmd` に持たせる（unwrap.rs）
//! - git のグローバルオプション（`-C`、`-c`、`--config-env`）は `Cmd` を作るときに読む。サブコマンドの引数は
//!   git_args.rs で、値を取るオプションの値をオプションと取り違えずに読む
//!
//! 展開（`$VAR`、`$(…)`）は行わず原文のまま扱う。bash 版と同じ

use std::path::{Path, PathBuf};
use tree_sitter::{Node, Parser, Tree};

use crate::git_args::{GitInvocation, parse_invocation};
use crate::unwrap::{EnvChange, basename, unwrap};

/// hook のエスケープ（`ALLOW_…=<理由>`）の読み取り（ADR-0023）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Escape {
    /// 代入が無い
    Absent,
    /// 代入はあるが、値が理由として短い（`1`、`true`、`force` など）
    Short,
    /// 理由つき
    Given,
}

/// 理由の幅。空白（ASCII）を全部除き、ASCII は 1、それ以外（日本語など）は 2 と数える。表示幅に近いが、
/// `unicode-width` のような依存は足さず ASCII か否かだけで決める。`force` / `allow`（5）は弾き、
/// 日本語 3 文字（6）から通す。中身は検査しない（品質のゲートではなく、boolean 的な値を弾くため。ADR-0023）。
/// 空白の扱いは git 側の pre-commit（scan-staged-secrets.sh の `tr -d '[:space:]'`）と揃える
fn reason_width(value: &str) -> usize {
    value
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .map(|c| if c.is_ascii() { 1 } else { 2 })
        .sum()
}

const REASON_MIN_WIDTH: usize = 6;

pub struct Shell {
    src: String,
    tree: Tree,
    /// 全コマンド（文書順）。parse のときに一度だけ読む
    cmds: Vec<Cmd>,
}

/// コマンドの引数 1 つ
#[derive(Debug, Clone)]
pub struct Arg {
    /// クォートを外した値（展開はしない）
    pub text: String,
    /// ノードの原文。`"$(cat <<EOF …)"` のように入れ子のヒアドキュメント本文も含む
    pub raw: String,
}

/// 単純コマンド 1 つ（`command` ノード）。包み（env など）はほどいてある
#[derive(Debug, Clone)]
pub struct Cmd {
    pub name: String,
    pub args: Vec<Arg>,
    /// コマンド先頭のバイト位置（文書順の比較に使う）
    pub start: usize,
    /// 引数に構文エラーがある、または直後に解析できない断片（閉じていないクォート等）が続く
    pub has_error: bool,
    /// 属するシェルのスコープ（最も近い `$(…)` / `(…)` / `<(…)` のノード ID）。トップレベルは None。
    /// これらの中で実行した `cd` は親のカレントディレクトリを変えない
    pub scope: Option<usize>,
    /// このコマンドだけに効く環境変数の変更を順に（前置きの `NAME=value` と、包んだ `env` の `NAME=value` /
    /// `-u NAME` / `-i`、`exec -c`）
    pub env: Vec<EnvChange>,
    /// ほどいた包みの、書いたままの名前（外側から順に）
    pub wrappers: Vec<String>,
    /// 包んだ `env -C <dir>` の行き先（順に、前のものからの相対）
    pub dirs: Vec<String>,
    /// git のコマンドなら、グローバルオプションの読み取り
    pub git: Option<GitInvocation>,
}

impl Shell {
    pub fn parse(src: &str) -> Shell {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_bash::LANGUAGE.into())
            .expect("tree-sitter-bash grammar");
        let tree = parser
            .parse(src, None)
            .expect("tree-sitter parse never fails");
        let mut shell = Shell {
            src: src.to_string(),
            tree,
            cmds: Vec::new(),
        };
        shell.cmds = shell.collect_commands();
        shell
    }

    fn text(&self, node: Node) -> &str {
        &self.src[node.byte_range()]
    }

    /// `heredoc_body` を除く全ノードを文書順（先行順）で走査する
    fn walk(&self, node: Node, visit: &mut dyn FnMut(Node)) {
        if node.kind() == "heredoc_body" {
            return;
        }
        visit(node);
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk(child, visit);
        }
    }

    /// 全コマンドを文書順で返す（コマンド置換・サブシェル・ERROR 配下も含む）
    pub fn commands(&self) -> &[Cmd] {
        &self.cmds
    }

    fn collect_commands(&self) -> Vec<Cmd> {
        let mut out = Vec::new();
        self.walk(self.tree.root_node(), &mut |n| {
            if n.kind() == "command"
                && let Some(c) = self.command(n)
            {
                out.push(c);
            }
        });
        out
    }

    fn command(&self, node: Node) -> Option<Cmd> {
        let name_node = node.child_by_field_name("name")?;
        let name = match name_node.child(0) {
            Some(inner) => self.unquote(inner).text,
            None => self.text(name_node).to_string(),
        };
        let mut args = Vec::new();
        let mut cursor = node.walk();
        for child in node.children_by_field_name("argument", &mut cursor) {
            args.push(self.unquote(child));
        }
        let trailing_error = node.next_sibling().is_some_and(|n| n.kind() == "ERROR");
        let mut scope = None;
        let mut p = node.parent();
        while let Some(n) = p {
            if matches!(
                n.kind(),
                "command_substitution" | "subshell" | "process_substitution"
            ) {
                scope = Some(n.id());
                break;
            }
            p = n.parent();
        }
        let mut env = Vec::new();
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "variable_assignment"
                && let Some(var) = child.child_by_field_name("name")
            {
                let value = child
                    .child_by_field_name("value")
                    .map(|v| self.unquote(v).text)
                    .unwrap_or_default();
                env.push(EnvChange::Set(self.text(var).to_string(), value));
            }
        }
        let inner = unwrap(&name, &args);
        env.extend(inner.env);
        let git = (basename(&inner.name) == "git").then(|| parse_invocation(&inner.args));
        Some(Cmd {
            name: inner.name,
            args: inner.args,
            start: node.start_byte(),
            has_error: node.has_error() || trailing_error,
            scope,
            env,
            dirs: inner.dirs,
            wrappers: inner.wrappers,
            git,
        })
    }

    /// ノードを引数として読む。クォートを外し、展開は原文のまま残す
    pub fn unquote(&self, node: Node) -> Arg {
        let raw = self.text(node).to_string();
        let text = match node.kind() {
            "word" | "number" => raw.clone(),
            "raw_string" => strip(&raw, "'", "'"),
            "ansi_c_string" => strip(&raw, "$'", "'"),
            "string" | "translated_string" => {
                let mut s = String::new();
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    s.push_str(self.text(child));
                }
                s
            }
            "concatenation" => {
                let mut s = String::new();
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    s.push_str(&self.unquote(child).text);
                }
                s
            }
            _ => raw.clone(),
        };
        Arg { text, raw }
    }

    /// プログラム内の全ての代入（`NAME=value`。値の無い `NAME=` は空の値）の名前と値
    pub fn assignments(&self) -> Vec<(String, String)> {
        let mut found = Vec::new();
        self.walk(self.tree.root_node(), &mut |n| {
            if n.kind() == "variable_assignment" {
                let var = n.child_by_field_name("name").map(|v| self.text(v));
                let val = n
                    .child_by_field_name("value")
                    .map(|v| self.unquote(v).text)
                    .unwrap_or_default();
                if let Some(var) = var {
                    found.push((var.to_string(), val));
                }
            }
        });
        found
    }

    /// `unset NAME` がプログラム内のどこかにあるか（unset は組み込みで、commands() には出ない）
    pub fn unsets(&self, name: &str) -> bool {
        let mut found = false;
        self.walk(self.tree.root_node(), &mut |n| {
            if n.kind() == "unset_command" {
                let mut cursor = n.walk();
                if n.named_children(&mut cursor).any(|c| self.text(c) == name) {
                    found = true;
                }
            }
        });
        found
    }

    /// `NAME=<値>` の代入をプログラム内から探す（hook のエスケープ指定）。値が理由として足りるかで分ける。
    /// `0` と空は付けていないものとして扱う。代入が複数あれば、短い値が 1 つでもあれば Short
    /// （後ろの代入で上書きして通せない）
    pub fn escape(&self, name: &str) -> Escape {
        let mut found = Escape::Absent;
        self.walk(self.tree.root_node(), &mut |n| {
            if n.kind() == "variable_assignment"
                && n.child_by_field_name("name").map(|v| self.text(v)) == Some(name)
            {
                let value = n
                    .child_by_field_name("value")
                    .map(|v| self.unquote(v).text)
                    .unwrap_or_default();
                let value = value.trim();
                let this = if value.is_empty() || value == "0" {
                    Escape::Absent
                } else if reason_width(value) >= REASON_MIN_WIDTH {
                    Escape::Given
                } else {
                    Escape::Short
                };
                found = match (found, this) {
                    (Escape::Short, _) | (_, Escape::Short) => Escape::Short,
                    (Escape::Given, _) | (_, Escape::Given) => Escape::Given,
                    _ => Escape::Absent,
                };
            }
        });
        found
    }

    /// コマンドが対象にするディレクトリ。そのコマンドより前にあり、同じシェルのスコープで実行される
    /// `cd <dir>` を順に畳み込み、包んだ `env -C <dir>` と `git -C <dir>` を順に当てる
    /// （`$(cd x && …)` や `(cd x)` の cd は親のディレクトリを変えないので見ない）
    pub fn target_dir(&self, cmd: &Cmd, base: &Path) -> PathBuf {
        let mut dir = base.to_path_buf();
        for c in &self.cmds {
            if c.start >= cmd.start {
                break;
            }
            if c.changes_dir() && c.scope == cmd.scope {
                match c.args.iter().find(|a| !a.text.starts_with('-')) {
                    Some(a) => dir = resolve(&dir, &a.text),
                    None => dir = home(),
                }
            }
        }
        for d in &cmd.dirs {
            dir = resolve(&dir, d);
        }
        if let Some(git) = &cmd.git {
            for d in &git.dirs {
                dir = resolve(&dir, d);
            }
        }
        dir
    }
}

fn strip(s: &str, open: &str, close: &str) -> String {
    s.strip_prefix(open)
        .and_then(|t| t.strip_suffix(close))
        .unwrap_or(s)
        .to_string()
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// `~` / `~/x` と相対パスを解決する（存在確認はしない）
fn resolve(base: &Path, p: &str) -> PathBuf {
    if p == "~" {
        return home();
    }
    if let Some(rest) = p.strip_prefix("~/") {
        return home().join(rest);
    }
    base.join(p)
}

impl Cmd {
    /// シェルの cd か。builtin / command と、先頭に修飾せずに書いた time（bash の予約語）はシェルの cd を
    /// 呼ぶ。env や /usr/bin/time など外部の包みは cd という外部コマンドを探して失敗し、シェルのディレクトリを
    /// 変えない。予約語は先頭でだけ予約語として読まれるので、command time は外部の time を呼ぶ
    pub fn changes_dir(&self) -> bool {
        self.name == "cd"
            && self
                .wrappers
                .iter()
                .enumerate()
                .all(|(i, w)| w == "builtin" || w == "command" || (i == 0 && w == "time"))
    }

    /// 走らせるプログラムの、書いたままのパス。`bash <path>` / `sh <path>` は走らせるスクリプトのパス
    pub fn program(&self) -> &str {
        let name = basename(&self.name);
        if name == "bash" || name == "sh" {
            return self.args.first().map_or(&self.name, |a| &a.text);
        }
        &self.name
    }

    /// `git [global opts] <sub> args…` の sub と args
    pub fn git_subcommand(&self) -> Option<(&str, &[Arg])> {
        let i = self.git.as_ref()?.sub?;
        Some((self.args[i].text.as_str(), &self.args[i + 1..]))
    }

    /// `git -c <設定>` / `--config-env <設定>` の設定（サブコマンドより前）
    pub fn git_config_overrides(&self) -> &[String] {
        self.git.as_ref().map_or(&[], |g| g.config.as_slice())
    }

    /// `gh <words…> args…` の args（例: `gh pr create`）
    pub fn gh_subcommand(&self, words: &[&str]) -> Option<&[Arg]> {
        if basename(&self.name) != "gh" || self.args.len() < words.len() {
            return None;
        }
        if self.args.iter().zip(words).all(|(a, w)| a.text == *w) {
            Some(&self.args[words.len()..])
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// 引数の読み取りヘルパー（旧 bash hook の has_flag / opt_value 相当）
// ---------------------------------------------------------------------------

/// いずれかのフラグが `name` または `name=…` の形で現れるか
pub fn has_flag(args: &[Arg], names: &[&str]) -> bool {
    args.iter().any(|a| {
        names
            .iter()
            .any(|n| a.text == *n || a.text.starts_with(&format!("{n}=")))
    })
}

/// フラグの値（`--x value` / `--x=value`）。最初に現れたもの
pub fn opt_value<'a>(args: &'a [Arg], names: &[&str]) -> Option<&'a Arg> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        for n in names {
            if a.text == *n {
                return it.next();
            }
        }
    }
    args.iter()
        .find(|a| names.iter().any(|n| a.text.starts_with(&format!("{n}="))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmds(src: &str) -> Vec<Cmd> {
        Shell::parse(src).commands().to_vec()
    }

    #[test]
    fn heredoc_inside_substitution_is_not_a_command() {
        let src = "gh pr create --title \"feat: x\" --body \"$(cat <<'EOF'\n## Summary\ngit push origin main\nEOF\n)\"";
        let cs = cmds(src);
        let names: Vec<&str> = cs.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["gh", "cat"]);
        let body = opt_value(
            cs[0].gh_subcommand(&["pr", "create"]).unwrap(),
            &["--body", "-b"],
        )
        .unwrap();
        assert!(body.raw.contains("## Summary"));
        assert!(!cs[0].has_error);
    }

    #[test]
    fn quoted_heredoc_operator_is_a_string() {
        let cs = cmds("echo \"a << b\"; git push origin main");
        assert_eq!(cs.len(), 2);
        assert_eq!(cs[1].git_subcommand().unwrap().0, "push");
    }

    #[test]
    fn unquote_variants() {
        let cs = cmds("x 'a b' \"c d\" e --title=\"f g\" -f query='h' $'i'");
        let t: Vec<&str> = cs[0].args.iter().map(|a| a.text.as_str()).collect();
        assert_eq!(t, ["a b", "c d", "e", "--title=f g", "-f", "query=h", "i"]);
        assert!(!cs[0].has_error);
    }

    #[test]
    fn expansions_stay_literal() {
        let cs = cmds("git add \"$FILE\" ${DIR}/x $(basename y)");
        let t: Vec<&str> = cs[0].args.iter().map(|a| a.text.as_str()).collect();
        assert_eq!(t, ["add", "$FILE", "${DIR}/x", "$(basename y)"]);
    }

    #[test]
    fn git_subcommand_skips_global_options() {
        let cs = cmds("git -C /repo -c core.autocrlf=false --no-pager commit -am x");
        let (sub, args) = cs[0].git_subcommand().unwrap();
        assert_eq!(sub, "commit");
        assert_eq!(args[0].text, "-am");
        assert!(cmds("git log --grep commit")[0].git_subcommand().unwrap().0 == "log");
    }

    #[test]
    fn escape_assignment_anywhere() {
        let esc = |src: &str| Shell::parse(src).escape("ALLOW_PROTECTED_PUSH");
        assert_eq!(
            esc("ALLOW_PROTECTED_PUSH=テスト用 git push --force"),
            Escape::Given
        );
        assert_eq!(
            esc("ALLOW_PROTECTED_PUSH=テスト用; git push"),
            Escape::Given
        );
        assert_eq!(esc("ALLOW_PROTECTED_PUSH=0 git push"), Escape::Absent);
        assert_eq!(esc("echo ALLOW_PROTECTED_PUSH=テスト用"), Escape::Absent);
    }

    // 値は理由の文字列で、空白を除いた幅（ASCII は 1、それ以外は 2）が 6 以上（ADR-0023）
    #[test]
    fn escape_needs_a_reason_of_width_six() {
        let esc = |v: &str| {
            Shell::parse(&format!("ALLOW_PROTECTED_PUSH={v} git push"))
                .escape("ALLOW_PROTECTED_PUSH")
        };
        assert!(matches!(esc("1"), Escape::Short));
        assert!(matches!(esc("force"), Escape::Short));
        assert!(matches!(esc("確認"), Escape::Short));
        assert!(matches!(esc("aaaaaa"), Escape::Given));
        assert!(matches!(esc("差分同一"), Escape::Given));
        assert!(matches!(esc("'  abcdef  '"), Escape::Given));
        assert!(matches!(
            Shell::parse("git push").escape("ALLOW_PROTECTED_PUSH"),
            Escape::Absent
        ));
        // 内側の空白（全角空白、NBSP を含む）で幅を水増しできない
        assert!(matches!(esc("'a    b'"), Escape::Short));
        assert!(matches!(esc("'a　　　b'"), Escape::Short));
        assert!(matches!(esc("'a\u{a0}\u{a0}\u{a0}\u{a0}b'"), Escape::Short));
        assert!(matches!(esc("'　　　'"), Escape::Absent));
        // 展開を含む値は、記録に理由が残らないので理由とみなさない
        assert!(matches!(esc("$REASON"), Escape::Short));
        assert!(matches!(esc("\"$(cat r.txt)\""), Escape::Short));
        assert!(matches!(esc("\"rebase のみ $X\""), Escape::Short));
        // 0 と空（空白だけを含む）は、付けていないものとして扱う
        assert!(matches!(esc("0"), Escape::Absent));
        assert!(matches!(esc("''"), Escape::Absent));
        assert!(matches!(esc("'     '"), Escape::Absent));
        // 同じ名前の代入が複数あれば、短い値が 1 つでもあれば短い扱い（後ろの代入で上書きできない）
        assert!(matches!(
            Shell::parse(
                "ALLOW_PROTECTED_PUSH=1 git push --force; ALLOW_PROTECTED_PUSH=placeholder true"
            )
            .escape("ALLOW_PROTECTED_PUSH"),
            Escape::Short
        ));
    }

    #[test]
    fn target_dir_prefers_git_c_then_preceding_cd() {
        let base = Path::new("/base");
        let sh = Shell::parse("cd /a && git -C /b commit");
        let cs = sh.commands();
        assert_eq!(sh.target_dir(&cs[1], base), PathBuf::from("/b"));

        let sh = Shell::parse("cd /a; cd sub && git commit && cd /z");
        let cs = sh.commands();
        let git = cs.iter().find(|c| c.name == "git").unwrap();
        assert_eq!(sh.target_dir(git, base), PathBuf::from("/a/sub"));

        let sh = Shell::parse("git commit && cd /z");
        let cs = sh.commands();
        assert_eq!(sh.target_dir(&cs[0], base), PathBuf::from("/base"));
    }

    #[test]
    fn cd_inside_substitution_or_subshell_does_not_change_parent_dir() {
        let base = Path::new("/base");
        let sh = Shell::parse("echo $(cd /a && pwd) && git commit");
        let cs = sh.commands();
        let git = cs.iter().find(|c| c.name == "git").unwrap();
        assert_eq!(sh.target_dir(git, base), PathBuf::from("/base"));

        let sh = Shell::parse("(cd /a) ; git commit");
        let cs = sh.commands();
        let git = cs.iter().find(|c| c.name == "git").unwrap();
        assert_eq!(sh.target_dir(git, base), PathBuf::from("/base"));

        // 同じサブシェルの中なら効く
        let sh = Shell::parse("(cd /a && git commit)");
        let cs = sh.commands();
        let git = cs.iter().find(|c| c.name == "git").unwrap();
        assert_eq!(sh.target_dir(git, base), PathBuf::from("/a"));
    }

    #[test]
    fn command_in_message_is_data() {
        let cs = cmds("git commit -m \"docs: run git add -A\"");
        assert_eq!(cs.len(), 1);
        assert_eq!(cs[0].git_subcommand().unwrap().0, "commit");
    }

    #[test]
    fn unbalanced_quote_marks_error() {
        // 閉じていないクォートは引数にならず、コマンドの直後に ERROR として残る
        assert!(cmds("git add \"unterminated")[0].has_error);
        assert!(cmds("git add 'x")[0].has_error);
        assert!(!cmds("git add 'x y'")[0].has_error);
    }

    #[test]
    fn helpers() {
        let cs = cmds("gh pr merge 12 --merge -d --subject=\"x\"");
        let args = cs[0].gh_subcommand(&["pr", "merge"]).unwrap();
        assert!(has_flag(args, &["--merge", "-m"]));
        assert!(has_flag(args, &["--delete-branch", "-d"]));
        assert_eq!(
            opt_value(args, &["--subject", "-t"]).unwrap().text,
            "--subject=x"
        );
        let am = cmds("git commit -am x");
        let (sub, args) = am[0].git_subcommand().unwrap();
        assert!(crate::git_args::git_flag(sub, args, Some('a'), &[]));
        let all = cmds("git commit --all");
        let (sub, args) = all[0].git_subcommand().unwrap();
        assert!(!crate::git_args::git_flag(sub, args, Some('a'), &[]));
    }
}
