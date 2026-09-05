//! Bash ツールのコマンド文字列の構文解析（tree-sitter-bash）
//!
//! 旧 bash hook が正規表現とヒアドキュメントのマスクで近似していたことを、AST で決定的に行う:
//! - `heredoc_body` はデータなので配下のコマンドを探さない（本文に書かれたコマンド例に反応しない）
//! - クォート・コマンド置換はノード種別で分かる（`"a << b"` はヒアドキュメントではない、
//!   `-m "git add -A"` はコマンドではない）
//! - `git -C <dir>` や先行する `cd <dir>` は AST 上の位置で判定する
//!
//! 展開（`$VAR`、`$(…)`）は行わず原文のまま扱う。bash 版と同じ

use std::path::{Path, PathBuf};
use tree_sitter::{Node, Parser, Tree};

pub struct Shell {
    src: String,
    tree: Tree,
}

/// コマンドの引数 1 つ
#[derive(Debug, Clone)]
pub struct Arg {
    /// クォートを外した値（展開はしない）
    pub text: String,
    /// ノードの原文。`"$(cat <<EOF …)"` のように入れ子のヒアドキュメント本文も含む
    pub raw: String,
}

/// 単純コマンド 1 つ（`command` ノード）
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
        Shell {
            src: src.to_string(),
            tree,
        }
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
    pub fn commands(&self) -> Vec<Cmd> {
        let mut out = Vec::new();
        self.walk(self.tree.root_node(), &mut |n| {
            if n.kind() == "command" {
                if let Some(c) = self.command(n) {
                    out.push(c);
                }
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
        Some(Cmd {
            name,
            args,
            start: node.start_byte(),
            has_error: node.has_error() || trailing_error,
            scope,
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

    /// `NAME=1` の代入がプログラム内のどこかにあるか（hook のエスケープ指定）
    pub fn has_escape(&self, name: &str) -> bool {
        let mut found = false;
        self.walk(self.tree.root_node(), &mut |n| {
            if n.kind() == "variable_assignment" {
                let var = n.child_by_field_name("name").map(|v| self.text(v));
                let val = n.child_by_field_name("value").map(|v| self.unquote(v).text);
                if var == Some(name) && val.as_deref() == Some("1") {
                    found = true;
                }
            }
        });
        found
    }

    /// コマンドが対象にするディレクトリ。同じコマンドの `git -C <dir>` が最優先、
    /// なければそのコマンドより前にあり、同じシェルのスコープで実行される `cd <dir>` を順に畳み込む
    /// （`$(cd x && …)` や `(cd x)` の cd は親のディレクトリを変えないので見ない）
    pub fn target_dir(&self, cmd: &Cmd, base: &Path) -> PathBuf {
        if cmd.name == "git" {
            let mut dir: Option<PathBuf> = None;
            let mut it = cmd.args.iter();
            while let Some(a) = it.next() {
                if !a.text.starts_with('-') {
                    break;
                }
                if a.text == "-C" {
                    if let Some(v) = it.next() {
                        let cur = dir.clone().unwrap_or_else(|| base.to_path_buf());
                        dir = Some(resolve(&cur, &v.text));
                    }
                } else if git_global_takes_value(&a.text) {
                    it.next();
                }
            }
            if let Some(d) = dir {
                return d;
            }
        }
        let mut dir = base.to_path_buf();
        for c in self.commands() {
            if c.start >= cmd.start {
                break;
            }
            if c.name == "cd" && c.scope == cmd.scope {
                match c.args.iter().find(|a| !a.text.starts_with('-')) {
                    Some(a) => dir = resolve(&dir, &a.text),
                    None => dir = home(),
                }
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

/// git のグローバルオプションのうち、次の引数を値として取るもの
fn git_global_takes_value(opt: &str) -> bool {
    matches!(
        opt,
        "-C" | "-c"
            | "--git-dir"
            | "--work-tree"
            | "--namespace"
            | "--config-env"
            | "--super-prefix"
            | "--list-cmds"
    )
}

impl Cmd {
    /// `git [global opts] <sub> args…` の sub と args
    pub fn git_subcommand(&self) -> Option<(&str, &[Arg])> {
        if self.name != "git" {
            return None;
        }
        let mut i = 0;
        while i < self.args.len() {
            let t = self.args[i].text.as_str();
            if !t.starts_with('-') {
                return Some((t, &self.args[i + 1..]));
            }
            i += if git_global_takes_value(t) { 2 } else { 1 };
        }
        None
    }

    /// `gh <words…> args…` の args（例: `gh pr create`）
    pub fn gh_subcommand(&self, words: &[&str]) -> Option<&[Arg]> {
        if self.name != "gh" || self.args.len() < words.len() {
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

/// `-am` のような短縮形クラスタに文字 ch が含まれるか（`--` で始まる長い形は対象外）
pub fn short_cluster_has(arg: &Arg, ch: char) -> bool {
    let t = &arg.text;
    t.starts_with('-') && !t.starts_with("--") && t[1..].contains(ch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmds(src: &str) -> Vec<Cmd> {
        Shell::parse(src).commands()
    }

    #[test]
    fn heredoc_inside_substitution_is_not_a_command() {
        let src = "gh pr create --title \"feat: x\" --body \"$(cat <<'EOF'\n## 概要\ngit push origin main\nEOF\n)\"";
        let cs = cmds(src);
        let names: Vec<&str> = cs.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["gh", "cat"]);
        let body = opt_value(
            cs[0].gh_subcommand(&["pr", "create"]).unwrap(),
            &["--body", "-b"],
        )
        .unwrap();
        assert!(body.raw.contains("## 概要"));
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
        assert!(
            Shell::parse("ALLOW_PROTECTED_PUSH=1 git push --force")
                .has_escape("ALLOW_PROTECTED_PUSH")
        );
        assert!(
            Shell::parse("ALLOW_PROTECTED_PUSH=1; git push").has_escape("ALLOW_PROTECTED_PUSH")
        );
        assert!(
            !Shell::parse("ALLOW_PROTECTED_PUSH=0 git push").has_escape("ALLOW_PROTECTED_PUSH")
        );
        assert!(!Shell::parse("echo ALLOW_PROTECTED_PUSH=1").has_escape("ALLOW_PROTECTED_PUSH"));
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
        assert!(short_cluster_has(
            &Arg {
                text: "-am".into(),
                raw: "-am".into()
            },
            'a'
        ));
        assert!(!short_cluster_has(
            &Arg {
                text: "--all".into(),
                raw: "--all".into()
            },
            'a'
        ));
    }
}
