//! Bash ツールのコマンド文字列の構文解析（tree-sitter-bash）
//!
//! 旧 bash hook が正規表現とヒアドキュメントのマスクで近似していたことを、AST で決定的に行う:
//! - `heredoc_body` はデータなので配下のコマンドを探さない（本文に書かれたコマンド例に反応しない）
//! - クォート・コマンド置換はノード種別で分かる（`"a << b"` はヒアドキュメントではない、
//!   `-m "git add -A"` はコマンドではない）
//! - `git -C <dir>` や先行する `cd <dir>` は AST 上の位置で判定する
//!
//! - コマンドを包むもの（`env`・`command`・`exec`・`nohup`・`time`）はほどき、中のコマンドと、前置きの代入や
//!   `env` が変える環境変数を `Cmd` に持たせる（規則ごとにほどかない）
//! - git のオプションの値（グローバルの `-C` / `-c` / `--config-env`、サブコマンドごとの値を取るオプション）は
//!   ここで読み、短縮形のまとまりの中の値をオプションと取り違えない
//!
//! 展開（`$VAR`、`$(…)`）は行わず原文のまま扱う。bash 版と同じ

use std::collections::VecDeque;
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
    /// このコマンドだけに効く環境変数の変更（前置きの `NAME=value` と、包んだ `env` の `NAME=value` / `-u NAME`）。
    /// 外すなら None
    pub env: Vec<(String, Option<String>)>,
    /// 包んだ `env -i` などで、受け継ぐ環境変数を全て外すか
    pub env_cleared: bool,
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
                env.push((self.text(var).to_string(), Some(value)));
            }
        }
        Some(unwrap(Cmd {
            name,
            args,
            start: node.start_byte(),
            has_error: node.has_error() || trailing_error,
            scope,
            env,
            env_cleared: false,
        }))
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

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// 引数のプログラムをそのまま走らせる前置きと、そのうち値を別の語で取るオプション（env と GNU time は
/// coreutils と time の man、exec は bash の組み込み）
const WRAPPERS: &[(&str, &[&str])] = &[
    ("command", &[]),
    ("env", &["-u", "--unset", "-C", "--chdir", "-a", "--argv0"]),
    ("exec", &["-a"]),
    ("nohup", &[]),
    ("time", &["-f", "--format", "-o", "--output"]),
];

/// `env -S <文字列>`（`-S<文字列>`、`--split-string=<文字列>` も）の文字列。env はこれを空白で語に分けて、
/// 続く語の前に置く
fn split_string(option: &str, words: &mut VecDeque<Arg>) -> Option<String> {
    match option {
        "-S" | "--split-string" => words.pop_front().map(|a| a.text),
        _ => option
            .strip_prefix("--split-string=")
            .or_else(|| option.strip_prefix("-S"))
            .map(str::to_string),
    }
}

/// 前置き（env など）をほどき、中のコマンドと、env が変える環境変数を持たせる。`command -v` / `-V` は
/// プログラムを探して表示するだけで実行しないので、ほどかない
fn unwrap(mut cmd: Cmd) -> Cmd {
    loop {
        let wrapper = basename(&cmd.name).to_string();
        let Some((_, valued)) = WRAPPERS.iter().find(|(w, _)| *w == wrapper) else {
            return cmd;
        };
        let mut words: VecDeque<Arg> = cmd.args.iter().cloned().collect();
        let mut env = Vec::new();
        let mut cleared = false;
        while let Some(front) = words.front() {
            let t = front.text.clone();
            if !(t.starts_with('-') || (wrapper == "env" && t.contains('='))) {
                break;
            }
            words.pop_front();
            if wrapper == "command" && (t == "-v" || t == "-V") {
                return cmd;
            }
            if wrapper == "env" {
                if let Some(split) = split_string(&t, &mut words) {
                    for part in split.split_whitespace().rev() {
                        words.push_front(Arg {
                            text: part.to_string(),
                            raw: part.to_string(),
                        });
                    }
                    continue;
                }
                if t == "-i" || t == "--ignore-environment" || t == "-" {
                    cleared = true;
                    continue;
                }
                if t == "-u" || t == "--unset" {
                    if let Some(v) = words.pop_front() {
                        env.push((v.text, None));
                    }
                    continue;
                }
                if let Some(name) = t
                    .strip_prefix("--unset=")
                    .or_else(|| t.strip_prefix("-u").filter(|n| !n.is_empty()))
                {
                    env.push((name.to_string(), None));
                    continue;
                }
                if !t.starts_with('-')
                    && let Some((name, value)) = t.split_once('=')
                {
                    env.push((name.to_string(), Some(value.to_string())));
                    continue;
                }
            }
            if valued.contains(&t.as_str()) {
                words.pop_front();
            }
        }
        let Some(program) = words.pop_front() else {
            return cmd;
        };
        cmd.name = program.text;
        cmd.args = words.into_iter().collect();
        cmd.env.extend(env);
        cmd.env_cleared |= cleared;
    }
}

/// git のグローバルオプションのうち、次の引数を値として取るもの
pub fn git_global_takes_value(opt: &str) -> bool {
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

    /// `git -c <設定>` / `-c<設定>` と `--config-env <設定>` / `--config-env=<設定>` の設定（サブコマンドより前）
    pub fn git_config_overrides(&self) -> Vec<&str> {
        let mut found = Vec::new();
        if self.name != "git" {
            return found;
        }
        let mut it = self.args.iter();
        while let Some(a) = it.next() {
            let t = a.text.as_str();
            if !t.starts_with('-') {
                break;
            }
            if t == "-c" || t == "--config-env" {
                if let Some(v) = it.next() {
                    found.push(v.text.as_str());
                }
            } else if let Some(v) = t
                .strip_prefix("--config-env=")
                .or_else(|| t.strip_prefix("-c"))
            {
                found.push(v);
            } else if git_global_takes_value(t) {
                it.next();
            }
        }
        found
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

/// git のサブコマンドの、値を取るオプション（git の各サブコマンドの man から、規則が見るものだけ）
struct ValueOptions {
    /// 次の語か、続けて書いた残りを値に取る短縮形
    shorts: &'static str,
    /// 続けて書いたときだけ値を取る短縮形（`-S<keyid>`、`-u<mode>`）
    optional_shorts: &'static str,
    /// 次の語を値に取る長い形（`=` で続けた形は 1 語なので数えない）
    longs: &'static [&'static str],
}

fn value_options(sub: &str) -> ValueOptions {
    match sub {
        "commit" => ValueOptions {
            shorts: "mFCct",
            optional_shorts: "Su",
            longs: &[
                "--message",
                "--file",
                "--reuse-message",
                "--reedit-message",
                "--author",
                "--date",
                "--template",
                "--fixup",
                "--squash",
                "--cleanup",
                "--trailer",
                "--pathspec-from-file",
            ],
        },
        "push" => ValueOptions {
            shorts: "o",
            optional_shorts: "",
            longs: &["--push-option", "--repo", "--receive-pack", "--exec"],
        },
        "add" => ValueOptions {
            shorts: "",
            optional_shorts: "",
            longs: &["--chmod", "--pathspec-from-file"],
        },
        _ => ValueOptions {
            shorts: "",
            optional_shorts: "",
            longs: &[],
        },
    }
}

/// git のサブコマンド `sub` の引数に、短縮形のフラグ `flag` があるか。短縮形のまとまり（`-am`）は、値を取る
/// オプションのところで読むのを止め（`-m"add a"` の a は値）、別の語で渡した値（`-m -n`）と `--` より後は見ない
pub fn git_short_flag(sub: &str, args: &[Arg], flag: char) -> bool {
    let options = value_options(sub);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let t = a.text.as_str();
        if t == "--" {
            return false;
        }
        if options.longs.contains(&t) {
            it.next();
            continue;
        }
        if !t.starts_with('-') || t.starts_with("--") {
            continue;
        }
        let cluster = &t[1..];
        for (i, ch) in cluster.char_indices() {
            if ch == flag {
                return true;
            }
            if options.shorts.contains(ch) {
                // 残りが無ければ次の語が値
                if i + ch.len_utf8() == cluster.len() {
                    it.next();
                }
                break;
            }
            if options.optional_shorts.contains(ch) {
                break;
            }
        }
    }
    false
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
        Shell::parse(src).commands()
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
        let am = cmds("git commit -am x");
        let (sub, args) = am[0].git_subcommand().unwrap();
        assert!(git_short_flag(sub, args, 'a'));
        let all = cmds("git commit --all");
        let (sub, args) = all[0].git_subcommand().unwrap();
        assert!(!git_short_flag(sub, args, 'a'));
    }
}
