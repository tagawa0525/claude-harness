//! Bash ツールのコマンド文字列の構文解析（tree-sitter-bash）
//!
//! 旧 bash hook が正規表現とヒアドキュメントのマスクで近似していたことを、AST で決定的に行う:
//! - `heredoc_body` はデータなので配下のコマンドを探さない（本文に書かれたコマンド例に反応しない）
//! - クォート・コマンド置換はノード種別で分かる（`"a << b"` はヒアドキュメントではない、
//!   `-m "git add -A"` はコマンドではない）
//! - `git -C <dir>` や先行する `cd <dir>` は AST 上の位置で判定する
//!
//! コマンドは解析のときに一度だけ読み、規則はそれを共有する:
//! - コマンドを包むもの（env、command、exec、nohup、time、nice、timeout、setsid、stdbuf、ionice、sudo、xargs、flock、
//!   chrt、taskset、watch、script、ssh、`bash [opts] <script>`）はほどき、中のコマンドと、前置きの代入や包みが
//!   変える環境変数・ディレクトリを `Cmd` に持たせる（unwrap.rs）
//! - sh -c で走らせる文字列（`bash -c '…'`、`eval`、`watch`、`ssh <接続先> …`）は、同じ構文解析を再帰的に当て、
//!   中のコマンドを `commands()` に含める。深さの上限は `MAX_DEPTH`（ADR-0025）
//! - 走らせるプログラムを決められないコマンド（`$CMD`、`source`、`bash -s`）は `Cmd::opaque` にし、
//!   `runs_program` が止める側に倒す
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
    /// 値に展開（`$VAR`、`$(…)`）や ANSI-C 文字列（`$'…'`）が含まれ、展開を除いた字面だけでは理由として短い。
    /// Short と同じく理由を求める。git の hook のように展開後の値しか見えない層には、この区別が要る
    Expanded,
    /// 理由つき
    Given,
}

/// 理由の幅。空白（Unicode の White_Space。全角空白、NBSP を含む）とバックスラッシュを除き、ASCII は 1、それ以外
/// （日本語など）は 2 と数える。表示幅に近いが、`unicode-width` のような依存は足さず ASCII か否かだけで決める。
/// `force` / `allow`（5）は弾き、日本語 3 文字（6）から通す。中身は検査しない（品質のゲートではなく、boolean 的な
/// 値を弾くため。ADR-0023）。git 側の pre-commit（scan-staged-secrets.sh）は同じ White_Space を UTF-8 のバイト列で除く
fn reason_width(value: &str) -> usize {
    value
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '\\')
        .map(|c| if c.is_ascii() { 1 } else { 2 })
        .sum()
}

/// 展開（`$VAR`、`$(…)`、`<(…)`、`$((…))`）か ANSI-C 文字列（`$'…'`）か。字面と渡る値が違うので、
/// 理由の幅には数えない
fn is_expansion(node: Node) -> bool {
    matches!(
        node.kind(),
        "simple_expansion"
            | "expansion"
            | "command_substitution"
            | "process_substitution"
            | "arithmetic_expansion"
            | "ansi_c_string"
    )
}

fn contains_expansion(node: Node) -> bool {
    if is_expansion(node) {
        return true;
    }
    let mut cursor = node.walk();
    node.children(&mut cursor).any(contains_expansion)
}

/// 複数の代入があるときの優先順位。値が記録に残らないもの（Expanded）、短いもの（Short）、理由つき、無し、の順に
/// 強い（後ろの代入で上書きして通せない）
fn rank(esc: Escape) -> u8 {
    match esc {
        Escape::Absent => 0,
        Escape::Given => 1,
        Escape::Short => 2,
        Escape::Expanded => 3,
    }
}

const REASON_MIN_WIDTH: usize = 6;

/// `bash -c '…'` のような文字列を再帰的に解析する深さの上限（ADR-0025）。超えた文字列は読まず、
/// そのコマンドを [`Cmd::opaque`] にする（通す側ではなく止める側に倒す）
const MAX_DEPTH: usize = 5;

pub struct Shell {
    src: String,
    tree: Tree,
    /// 全コマンド（文書順）。parse のときに一度だけ読む。`bash -c` などの文字列の中のコマンドも、その位置に含む
    cmds: Vec<Cmd>,
    /// `bash -c` などの文字列を解析した内側のシェル。コマンドは `cmds` に移してある。代入・unset・エスケープの
    /// 走査に使い、スコープにノード ID を使うので木を生かしておく
    inner: Vec<Shell>,
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
    /// 属するシェルのスコープ（外側から順に、`$(…)` / `(…)` / `<(…)` と、`bash -c` などの文字列のノード ID）。
    /// トップレベルは空。これらの中で実行した `cd` は親のカレントディレクトリを変えない。前の `cd` が効くのは、
    /// その `cd` のスコープがこのスコープの前置きのとき（同じか、外側）
    pub scope: Vec<usize>,
    /// 走らせるプログラムを文字列から決められない: 名前の basename が展開（`$CMD`、`$(which x)`）、標準入力や
    /// ファイルからコマンドを読む（`bash -s`、`source`。unwrap.rs）、文字列の入れ子が深さの上限を超えた
    pub opaque: bool,
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
        Shell::parse_at(src, 0)
    }

    /// `depth` は文字列の入れ子の深さ（トップレベルは 0）
    fn parse_at(src: &str, depth: usize) -> Shell {
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
            inner: Vec::new(),
        };
        let (cmds, inner) = shell.collect_commands(depth);
        shell.cmds = cmds;
        shell.inner = inner;
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

    /// 全コマンド（文字列の中のものを含む）と、文字列を解析した内側のシェル
    fn collect_commands(&self, depth: usize) -> (Vec<Cmd>, Vec<Shell>) {
        let mut cmds = Vec::new();
        let mut inner = Vec::new();
        self.walk(self.tree.root_node(), &mut |n| {
            if n.kind() == "command"
                && let Some((cs, shell)) = self.command(n, depth)
            {
                cmds.extend(cs);
                inner.extend(shell);
            }
        });
        (cmds, inner)
    }

    /// `command` ノードのコマンドと、それが sh -c で走らせる文字列の中のコマンド（文書順）。文字列を解析した
    /// 内側のシェルも返す
    fn command(&self, node: Node, depth: usize) -> Option<(Vec<Cmd>, Option<Shell>)> {
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
        let mut scope = Vec::new();
        let mut p = node.parent();
        while let Some(n) = p {
            if matches!(
                n.kind(),
                "command_substitution" | "subshell" | "process_substitution"
            ) {
                scope.push(n.id());
            }
            p = n.parent();
        }
        scope.reverse();
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
        let unwrapped = unwrap(&name, &args);
        env.extend(unwrapped.env);
        let git = (basename(&unwrapped.name) == "git").then(|| parse_invocation(&unwrapped.args));
        // `$HOME/x/slurm-run.sh` は basename が字面なので決まる。`$CMD`、`~/x/$SCRIPT`、`$(which x)` は決まらない
        let opaque = unwrapped.opaque || basename(&unwrapped.name).contains(['$', '`']);
        let mut cmd = Cmd {
            name: unwrapped.name,
            args: unwrapped.args,
            start: node.start_byte(),
            has_error: node.has_error() || trailing_error,
            scope,
            opaque,
            env,
            dirs: unwrapped.dirs,
            wrappers: unwrapped.wrappers,
            git,
        };
        let Some(source) = unwrapped.source else {
            return Some((vec![cmd], None));
        };
        if depth >= MAX_DEPTH {
            cmd.opaque = true;
            return Some((vec![cmd], None));
        }
        let mut shell = Shell::parse_at(&source, depth + 1);
        let mut cmds = Vec::with_capacity(1 + shell.cmds.len());
        for mut c in std::mem::take(&mut shell.cmds) {
            // 文字列は外側のコマンドの原文より短いので、位置は外側のコマンドの後ろ、次の外側のコマンドの前に収まる
            c.start += node.start_byte() + 1;
            // 文字列の中は 1 つのスコープ。外側の cd は効き、中の cd は外に漏れない
            let mut scope = cmd.scope.clone();
            scope.push(node.id());
            scope.extend(c.scope);
            c.scope = scope;
            // 外側の環境変数とディレクトリの変更は、中のコマンドにも効く。外側の `env -C` は中の `cd` より後に
            // 当たる（`env -C x bash -c 'cd y && …'` は x/y でなく y/x になる。まれな形なので、順序は持たない）
            c.env = cmd.env.iter().cloned().chain(c.env).collect();
            c.dirs = cmd.dirs.iter().cloned().chain(c.dirs).collect();
            cmds.push(c);
        }
        cmds.insert(0, cmd);
        Some((cmds, Some(shell)))
    }

    /// ノードを引数として読む。クォートとバックスラッシュのエスケープを外し、展開は原文のまま残す
    pub fn unquote(&self, node: Node) -> Arg {
        let raw = self.text(node).to_string();
        let text = match node.kind() {
            "word" => unescape_word(&raw),
            "number" => raw.clone(),
            "raw_string" => strip(&raw, "'", "'"),
            "ansi_c_string" => strip(&raw, "$'", "'"),
            "string" | "translated_string" => {
                let mut s = String::new();
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    if child.kind() == "string_content" {
                        s.push_str(&unescape_double(self.text(child)));
                    } else {
                        s.push_str(self.text(child));
                    }
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

    /// `is_target` を満たすプログラムを走らせるコマンドがあるか。走らせるプログラムを決められないコマンド
    /// （[`Cmd::opaque`]）があれば、コマンド文字列のどこかに `word` が現れるときに、走らせうるものとして
    /// 止める側に倒す（`CMD=sbatch; $CMD`、`eval "$CMD"`、`source run-sbatch.sh`。ADR-0025）。引数に現れる
    /// だけの `echo sbatch` や `grep sbatch` は、決められないコマンドが無ければ対象外
    pub fn runs_program(&self, word: &str, is_target: impl Fn(&str) -> bool) -> bool {
        self.cmds.iter().any(|c| is_target(c.program()))
            || (self.cmds.iter().any(|c| c.opaque) && self.src.contains(word))
    }

    /// プログラム内（文字列の中を含む）の全ての代入（`NAME=value`。値の無い `NAME=` は空の値）の名前と値
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
        found.extend(self.inner.iter().flat_map(Shell::assignments));
        found
    }

    /// `unset NAME` がプログラム内（文字列の中を含む）のどこかにあるか（unset は組み込みで、commands() には出ない）
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
        found || self.inner.iter().any(|s| s.unsets(name))
    }

    /// `NAME=<値>` の代入をプログラム内から探す（hook のエスケープ指定）。値が理由として足りるかで分ける。
    /// `0` と空は付けていないものとして扱う。代入が複数あれば、短い値が 1 つでもあれば Short
    /// （後ろの代入で上書きして通せない）
    /// 値の字面のうち、展開と ANSI-C 文字列を除いた部分。記録にそのまま残る理由で、幅はこれで数える
    /// （PR 番号などは展開で足せる）
    fn literal(&self, node: Node) -> String {
        if is_expansion(node) {
            return String::new();
        }
        match node.kind() {
            "raw_string" => strip(self.text(node), "'", "'"),
            "string" | "translated_string" | "concatenation" => {
                let mut s = String::new();
                let mut cursor = node.walk();
                for child in node.named_children(&mut cursor) {
                    s.push_str(&self.literal(child));
                }
                s
            }
            _ => self.text(node).to_string(),
        }
    }

    pub fn escape(&self, name: &str) -> Escape {
        let mut found = Escape::Absent;
        self.walk(self.tree.root_node(), &mut |n| {
            if n.kind() == "variable_assignment"
                && n.child_by_field_name("name").map(|v| self.text(v)) == Some(name)
            {
                let value_node = n.child_by_field_name("value");
                let literal = value_node.map(|v| self.literal(v)).unwrap_or_default();
                let literal = literal.trim();
                let expanded = value_node.is_some_and(contains_expansion);
                let this = if !expanded && (literal.is_empty() || literal == "0") {
                    Escape::Absent
                } else if reason_width(literal) >= REASON_MIN_WIDTH {
                    Escape::Given
                } else if expanded {
                    Escape::Expanded
                } else {
                    Escape::Short
                };
                if rank(this) > rank(found) {
                    found = this;
                }
            }
        });
        // 文字列の中（`bash -c 'ALLOW_X=理由 git push'`）の代入も、同じ順位で合わせる
        for this in self.inner.iter().map(|s| s.escape(name)) {
            if rank(this) > rank(found) {
                found = this;
            }
        }
        found
    }

    /// コマンドが対象にするディレクトリ。そのコマンドより前にあり、同じか外側のシェルのスコープで実行される
    /// `cd <dir>` を順に畳み込み、包んだ `env -C <dir>` と `git -C <dir>` を順に当てる
    /// （`$(cd x && …)` や `(cd x)` の cd は親のディレクトリを変えないので見ない）
    pub fn target_dir(&self, cmd: &Cmd, base: &Path) -> PathBuf {
        let mut dir = base.to_path_buf();
        for c in &self.cmds {
            if c.start >= cmd.start {
                break;
            }
            if c.changes_dir() && cmd.scope.starts_with(&c.scope) {
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

/// クォートの外のバックスラッシュを外す（`\x` は x。`\<改行>` は行の継続で、消える）。tree-sitter-bash は
/// `a\ b` を 1 つの `word` にし、文字はそのまま持つ
fn unescape_word(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\n') => {}
            Some(n) => out.push(n),
            None => out.push('\\'),
        }
    }
    out
}

/// 二重引用符の中のバックスラッシュを外す。bash が特別に扱うのは `$`、`` ` ``、`"`、`\`、改行の前だけで、
/// ほかの `\x` はそのまま残る
fn unescape_double(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some(n @ ('$' | '`' | '"' | '\\')) => out.push(n),
            Some('\n') => {}
            Some(n) => {
                out.push('\\');
                out.push(n);
            }
            None => out.push('\\'),
        }
    }
    out
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

    /// 走らせるプログラムの、書いたままのパス。包み（env、`bash [opts] <path>` など）はほどいてある
    pub fn program(&self) -> &str {
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
        // EM SPACE（U+2003）のような Unicode の空白も除く
        assert!(matches!(
            esc("'a\u{2003}\u{2003}\u{2003}\u{2003}b'"),
            Escape::Short
        ));
        // ANSI-C 文字列とバックスラッシュは、字面の長さと渡る値が違うので数えない
        assert!(matches!(esc("$'\\t\\t\\t'"), Escape::Expanded));
        assert!(matches!(esc("$'abc'"), Escape::Expanded));
        assert!(matches!(esc("a\\ \\ \\ \\ b"), Escape::Short));
        // 展開は記録に残らないので幅に数えない。字面だけで足りなければ Expanded（展開後しか見えない層のために
        // Short と区別する）、足りれば Given
        assert!(matches!(esc("$REASON"), Escape::Expanded));
        assert!(matches!(esc("\"$(cat r.txt)\""), Escape::Expanded));
        assert!(matches!(esc("\"ab $X\""), Escape::Expanded));
        assert!(matches!(esc("ab$X"), Escape::Expanded));
        assert!(matches!(esc("<(true)"), Escape::Expanded));
        assert!(matches!(esc("$((1+1))"), Escape::Expanded));
        assert!(matches!(esc("\"rebase のみ $X\""), Escape::Given));
        assert!(matches!(
            esc("\"rebase のみ、PR #$(gh pr view)\""),
            Escape::Given
        ));
        assert!(matches!(esc("abcdef$X"), Escape::Given));
        assert!(matches!(
            Shell::parse("ALLOW_PROTECTED_PUSH=$R git push; ALLOW_PROTECTED_PUSH=1 true")
                .escape("ALLOW_PROTECTED_PUSH"),
            Escape::Expanded
        ));
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

    fn names(cs: &[Cmd]) -> Vec<&str> {
        cs.iter().map(|c| c.name.as_str()).collect()
    }

    /// bash の二重引用符の文字列として 1 段包む（`"` と `\` をエスケープする）
    fn wrap(src: &str) -> String {
        format!(
            "bash -c \"{}\"",
            src.replace('\\', "\\\\").replace('"', "\\\"")
        )
    }

    #[test]
    fn bash_options_are_skipped_before_the_script() {
        for src in [
            "bash -x s.sh a",
            "bash -e s.sh a",
            "sh -eu s.sh a",
            "bash -o pipefail s.sh a",
            "bash --rcfile r s.sh a",
            "bash -- s.sh a",
            "bash +O extglob s.sh a",
            "bash -xeu -o errexit -- s.sh a",
            "nohup bash -x s.sh a",
        ] {
            let cs = cmds(src);
            assert_eq!(cs[0].program(), "s.sh", "{src}");
            assert_eq!(cs[0].args[0].text, "a", "{src}");
            assert!(!cs[0].opaque, "{src}");
        }
        // -n は構文を調べるだけで走らせない
        assert_eq!(cmds("bash -n s.sh")[0].program(), "bash");
    }

    #[test]
    fn shell_strings_are_parsed_recursively() {
        let cs = cmds("bash -c 'cd /a && git commit --no-verify -m x' arg0");
        assert_eq!(names(&cs), ["bash", "cd", "git"]);
        assert_eq!(cs[2].git_subcommand().unwrap().0, "commit");
        assert!(cs[2].args.iter().any(|a| a.text == "--no-verify"));
        // 文字列の中のコマンドは、外側の bash より後ろ、次の外側のコマンドより前の位置を持つ
        let cs = cmds("bash -ec 'sbatch' && squeue");
        assert_eq!(names(&cs), ["bash", "sbatch", "squeue"]);
        assert!(cs[0].start < cs[1].start && cs[1].start < cs[2].start);
        // 入れ子
        let cs = cmds("sh -c \"bash -c 'sbatch'\"");
        assert_eq!(names(&cs), ["sh", "bash", "sbatch"]);
        // 外側の環境変数とディレクトリの変更は、中のコマンドに効く
        let cs = cmds("env -u CLAUDECODE -C /x FOO=1 bash -c 'git commit'");
        assert_eq!(
            cs[1].env,
            [
                EnvChange::Unset("CLAUDECODE".to_string()),
                EnvChange::Set("FOO".to_string(), "1".to_string())
            ]
        );
        assert_eq!(cs[1].dirs, ["/x"]);
        // watch、ssh、eval は残りの語を空白でつないだ文字列を sh -c で走らせる
        assert_eq!(
            names(&cmds("watch -n 5 'squeue; sbatch'")),
            ["watch", "squeue", "sbatch"]
        );
        assert_eq!(names(&cmds("ssh r995 sbatch --wrap x")), ["ssh", "sbatch"]);
        assert_eq!(names(&cmds("eval git commit -n")), ["eval", "git"]);
        assert_eq!(
            names(&cmds("flock /tmp/l -c 'sbatch'")),
            ["flock", "sbatch"]
        );
    }

    #[test]
    fn backslash_escapes_are_removed_like_bash() {
        let cs = cmds(r#"x a\ b "c\"d" 'e\f' g\\h "i\$j" "k\l""#);
        let t: Vec<&str> = cs[0].args.iter().map(|a| a.text.as_str()).collect();
        assert_eq!(t, ["a b", "c\"d", "e\\f", "g\\h", "i$j", "k\\l"]);
        // printf %q の形で入れ子にした文字列も、1 段ずつ読める
        let cs = cmds(r"bash -c bash\ -c\ sbatch\\\ --wrap\\\ true");
        assert_eq!(names(&cs), ["bash", "bash", "sbatch"]);
    }

    #[test]
    fn nesting_beyond_the_depth_limit_is_opaque() {
        let five = (0..5).fold("sbatch".to_string(), |s, _| wrap(&s));
        let sh = Shell::parse(&five);
        assert!(sh.commands().iter().any(|c| c.program() == "sbatch"));
        assert!(sh.commands().iter().all(|c| !c.opaque));
        let six = wrap(&five);
        let sh = Shell::parse(&six);
        assert!(!sh.commands().iter().any(|c| c.program() == "sbatch"));
        assert!(sh.commands().iter().any(|c| c.opaque));
        assert!(sh.runs_program("sbatch", |p| p == "sbatch"));
        assert!(!sh.runs_program("squeue", |p| p == "squeue"));
    }

    #[test]
    fn opaque_forms_run_the_word_when_it_appears_anywhere() {
        let runs =
            |src: &str| Shell::parse(src).runs_program("sbatch", |p| basename(p) == "sbatch");
        for src in [
            "CMD=sbatch; $CMD --wrap x",
            "source run-sbatch.sh",
            ". ./run-sbatch.sh",
            "bash -s < run-sbatch.sh",
            "bash < run-sbatch.sh",
            "echo 'sbatch --wrap x' | sh",
            "$(which sbatch) --wrap x",
            "CMD='sbatch --wrap x'; eval \"$CMD\"",
            "bash \"$SCRIPT\" sbatch",
            "ssh r995 < run-sbatch.sh",
            "sudo -s < run-sbatch.sh",
            "script log.txt < run-sbatch.sh",
            "sbatch --wrap x",
        ] {
            assert!(runs(src), "{src}");
        }
        for src in [
            "$CMD --wrap x",
            "eval \"$CMD\"",
            "source venv/bin/activate",
            "bash -s < run.sh",
            "ssh r995",
            "echo sbatch",
            "grep sbatch f",
            "bash -c 'echo sbatch'",
        ] {
            assert!(!runs(src), "{src}");
        }
        // 名前に展開があっても、basename が字面ならプログラムは決まる
        let cs = cmds("$HOME/.claude/scripts/slurm-run.sh -t 5");
        assert_eq!(cs[0].program(), "$HOME/.claude/scripts/slurm-run.sh");
        assert!(!cs[0].opaque);
        assert!(cmds("~/x/$SCRIPT")[0].opaque);
        assert!(cmds("`which sbatch` x")[0].opaque);
    }

    #[test]
    fn assignments_and_escapes_inside_shell_strings_are_read() {
        let sh = Shell::parse("bash -c 'CLAUDECODE=0; git commit'");
        assert!(
            sh.assignments()
                .contains(&("CLAUDECODE".to_string(), "0".to_string()))
        );
        assert!(Shell::parse("bash -c 'unset CLAUDECODE; git commit'").unsets("CLAUDECODE"));
        assert_eq!(
            Shell::parse("bash -c 'ALLOW_PROTECTED_PUSH=テスト用 git push'")
                .escape("ALLOW_PROTECTED_PUSH"),
            Escape::Given
        );
        assert_eq!(
            Shell::parse("ALLOW_PROTECTED_PUSH=テスト用 bash -c 'ALLOW_PROTECTED_PUSH=1 git push'")
                .escape("ALLOW_PROTECTED_PUSH"),
            Escape::Short
        );
    }

    #[test]
    fn preceding_cd_applies_inside_subshells_and_shell_strings() {
        let base = Path::new("/base");
        let dir_of_git = |src: &str| {
            let sh = Shell::parse(src);
            let git = sh.commands().iter().find(|c| c.name == "git").unwrap();
            sh.target_dir(git, base)
        };
        assert_eq!(dir_of_git("cd /a; (git commit)"), PathBuf::from("/a"));
        assert_eq!(
            dir_of_git("cd /a && echo $(git commit)"),
            PathBuf::from("/a")
        );
        assert_eq!(
            dir_of_git("cd /a; bash -c 'cd b && git commit'"),
            PathBuf::from("/a/b")
        );
        assert_eq!(
            dir_of_git("bash -c 'cd /a'; git commit"),
            PathBuf::from("/base")
        );
        assert_eq!(
            dir_of_git("env -C /x bash -c 'git commit'"),
            PathBuf::from("/x")
        );
        assert_eq!(dir_of_git("(cd /a); git commit"), PathBuf::from("/base"));
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
