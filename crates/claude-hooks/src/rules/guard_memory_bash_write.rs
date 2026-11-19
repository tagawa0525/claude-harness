//! guard-memory-bash-write: 自動メモリのディレクトリのファイルを Bash で書き換えるコマンドを止める
//!
//! memory（`<設定の場所>/projects/<プロジェクト>/memory/`。設定の場所は `CLAUDE_CONFIG_DIR`、無ければ `~/.claude`）の
//! ファイルを Bash で書き換えると、Claude Code が Read で読んだ状態と食い違い、次の Edit が古い内容の上に書く（#356）。
//! Read してから Write か Edit で直させる。
//!
//! 見る書き込み先は、モデルが普通に書く形（ADR-0042）: 出力のリダイレクト（`>`、`>>` など）の先、`tee` の引数、
//! `sed -i` と `perl -i` の対象のファイル、`cp`、`mv`、`install` の行き先、`truncate` の対象。
//! 消すこと（`rm`、元が memory の `mv`）は見ない: 読んだ状態との食い違いを作らず（次の Edit は無いファイルで失敗する）、
//! /handoff は済んだ引き継ぎのファイルを消す手順を持ち、Write と Edit ではファイルを消せない。
//! パスは、同じ呼び出しで字面のまま決まる変数と `HOME`、`CLAUDE_CONFIG_DIR` で展開し、コマンドの行き先
//! （前の `cd` と `env -C`。[`Shell::target_dir`] が書いたままの `~` と絶対パスで解き、変数は展開しない）からの
//! 相対で解き、`.` と `..` を字面で解く。値の決まらない展開を
//! 含むパス、python や perl の本文の中の `open`、ssh と `sudo -i` の先のコマンドは見ない（構文から手元の行き先が
//! 決まらない）。symlink は辿らない。読むだけのコマンド（`cat`、`grep`、入力のリダイレクト）は書き込み先を持たない
//!
//! エスケープ: コマンドに `ALLOW_MEMORY_BASH_WRITE=<理由>` を付ける。

use super::{Rule, escape_hint};
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Arg, Cmd, Expanded, Piece, Shell, home, resolve};
use crate::unwrap::basename;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

pub struct GuardMemoryBashWrite;

const ESCAPE: &str = "ALLOW_MEMORY_BASH_WRITE";

impl Rule for GuardMemoryBashWrite {
    fn name(&self) -> &'static str {
        "guard-memory-bash-write"
    }

    fn escape(&self) -> Option<&'static str> {
        Some(ESCAPE)
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        let config = std::env::var("CLAUDE_CONFIG_DIR")
            .ok()
            .filter(|v| !v.is_empty());
        let paths = memory_writes(shell, &input.cwd, &home(), config.as_deref());
        if paths.is_empty() {
            return Vec::new();
        }
        vec![Finding::Deny(message(&paths))]
    }
}

/// 止める文言（何を止めたか、正規の修正方法、エスケープの付け方）
fn message(paths: &[PathBuf]) -> String {
    let paths: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
    format!(
        "{} は自動メモリ（memory）のファイルです。Bash で書き換えると、Claude Code が Read で読んだ状態と\
         食い違い、次の Edit が古い内容の上に書きます。Read で読んでから、Write か Edit で直してください。{}",
        paths.join("、"),
        escape_hint(ESCAPE)
    )
}

/// shell のコマンドが書き込み先にする、memory のディレクトリの下のパス（文書順、重複なし）。`config` は
/// `CLAUDE_CONFIG_DIR` の値（無ければ `home` の `.claude`）
fn memory_writes(shell: &Shell, cwd: &Path, home: &Path, config: Option<&str>) -> Vec<PathBuf> {
    let config_dir = normalize(&match config {
        Some(c) => resolve(home, cwd, c),
        None => home.join(".claude"),
    });
    let vars = vars(shell, home, config);
    let mut found = Vec::new();
    for cmd in shell.commands() {
        if shell.runs_under_ssh_or_login(cmd) {
            continue;
        }
        // target_dir は前のコマンドを全部たどるので、書き込み先のあるコマンドだけで求める
        let targets = targets(cmd);
        if targets.is_empty() {
            continue;
        }
        let dir = shell.target_dir(cmd, cwd);
        for target in targets {
            let Expanded::Known(t) = target.expand(&vars) else {
                continue;
            };
            if t.is_empty() {
                continue;
            }
            // bash はクォートで始まる語の先頭の字面の `~` を展開しない（`resolve` は字面の `~/` をホームにする）。
            // 変数の値の先頭の `~`（`d=~/x` の代入で bash が展開済み）は当たらない
            let quoted_tilde = target.raw.starts_with(['"', '\''])
                && matches!(
                    // 二重引用符の値は、開きの引用符と最初の子の間の字面（空のことがある）から始まる
                    target.pieces.iter().find(|p| **p != Piece::Lit(String::new())),
                    Some(Piece::Lit(s)) if s.starts_with('~')
                );
            let t = if quoted_tilde { format!("./{t}") } else { t };
            let path = normalize(&resolve(home, &dir, &t));
            if in_memory(&path, &config_dir) && !found.contains(&path) {
                found.push(path);
            }
        }
    }
    found
}

/// パスの展開に使う変数。字面で決まる代入（[`Shell::literal_vars`]）に、コマンドの中で代入していなければ hook の
/// 環境の `HOME` と `CLAUDE_CONFIG_DIR` を足す（Claude Code が hook と Bash に同じ環境を渡す）
fn vars(shell: &Shell, home: &Path, config: Option<&str>) -> HashMap<String, String> {
    let mut vars = shell.literal_vars();
    let assigned: Vec<String> = shell.assignments().into_iter().map(|(n, _)| n).collect();
    let mut add = |name: &str, value: String| {
        if !assigned.iter().any(|n| n == name) {
            vars.insert(name.to_string(), value);
        }
    };
    add("HOME", home.display().to_string());
    if let Some(c) = config {
        add("CLAUDE_CONFIG_DIR", c.to_string());
    }
    vars
}

/// path が `config_dir/projects/<プロジェクト>/memory` か、その下か
fn in_memory(path: &Path, config_dir: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(config_dir) else {
        return false;
    };
    let mut c = rel.components();
    matches!(
        (c.next(), c.next(), c.next()),
        (Some(Component::Normal(p)), Some(Component::Normal(_)), Some(Component::Normal(m)))
            if p == "projects" && m == "memory"
    )
}

/// `.` と `..` を字面で解く
fn normalize(path: &Path) -> PathBuf {
    let mut norm = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                norm.pop();
            }
            c => norm.push(c),
        }
    }
    norm
}

/// コマンドの書き込み先（出力のリダイレクトの先と、プログラムが書くファイル）
fn targets(cmd: &Cmd) -> Vec<Arg> {
    let mut out = cmd.writes.clone();
    let args = &cmd.args;
    match basename(cmd.program()) {
        "tee" => out.extend(parse(args, &Spec::default()).operands),
        "truncate" => {
            let spec = Spec {
                valued: "sr",
                long_valued: &["--size", "--reference"],
                ..Spec::default()
            };
            out.extend(parse(args, &spec).operands);
        }
        name @ ("cp" | "mv" | "install") => {
            let spec = if name == "install" {
                Spec {
                    valued: "gmoSt",
                    long_valued: &[
                        "--group",
                        "--mode",
                        "--owner",
                        "--suffix",
                        "--target-directory",
                        "--strip-program",
                    ],
                    ..Spec::default()
                }
            } else {
                Spec {
                    valued: "St",
                    long_valued: &["--suffix", "--target-directory"],
                    ..Spec::default()
                }
            };
            let p = parse(args, &spec);
            // install -d は引数のディレクトリを作るだけで、ファイルを書かない
            if name == "install" && p.has(&["-d", "--directory"]) {
                return out;
            }
            match p.value(&["-t", "--target-directory"]) {
                Some(dir) => out.push(dir.clone()),
                None if p.operands.len() >= 2 => out.extend(p.operands.last().cloned()),
                None => {}
            }
        }
        "sed" => {
            let spec = Spec {
                valued: "efl",
                attached: "i",
                long_valued: &["--expression", "--file", "--line-length"],
            };
            let p = parse(args, &spec);
            if p.has(&["-i", "--in-place"]) {
                let script_given = p.has(&["-e", "-f", "--expression", "--file"]);
                out.extend(p.operands.into_iter().skip(usize::from(!script_given)));
            }
        }
        "perl" => {
            // 値を続けて書く短いオプション（-i の拡張子、-I のディレクトリ、-M のモジュールなど）は、まとめた残りを値に取る
            let spec = Spec {
                valued: "eE",
                attached: "iIMmx0CdDFV",
                long_valued: &[],
            };
            let p = parse(args, &spec);
            if p.has(&["-i"]) {
                let code_given = p.has(&["-e", "-E"]);
                out.extend(p.operands.into_iter().skip(usize::from(!code_given)));
            }
        }
        _ => {}
    }
    out
}

/// オプションの読み方。`valued` は値を取る短いオプション（まとめた残りか、次の引数）、`attached` は、まとめた残りを
/// 値に取る短いオプション（空でもよく、次の引数は取らない）、`long_valued` は、`=` が無ければ次の引数を値に取る長い
/// オプション
#[derive(Default)]
struct Spec {
    valued: &'static str,
    attached: &'static str,
    long_valued: &'static [&'static str],
}

/// 引数をオプション（`-x` か `--name` の形の名前と値）とオペランドに分けたもの
struct Parsed {
    opts: Vec<(String, Option<Arg>)>,
    operands: Vec<Arg>,
}

impl Parsed {
    fn has(&self, names: &[&str]) -> bool {
        self.opts.iter().any(|(n, _)| names.contains(&n.as_str()))
    }

    fn value(&self, names: &[&str]) -> Option<&Arg> {
        self.opts
            .iter()
            .rev()
            .find(|(n, v)| names.contains(&n.as_str()) && v.is_some())
            .and_then(|(_, v)| v.as_ref())
    }
}

fn parse(args: &[Arg], spec: &Spec) -> Parsed {
    let mut p = Parsed {
        opts: Vec::new(),
        operands: Vec::new(),
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let t = a.text.as_str();
        if t == "--" {
            p.operands.extend(it.cloned());
            break;
        }
        if let Some(long) = t.strip_prefix("--") {
            let opt = match long.split_once('=') {
                Some((name, _)) => (format!("--{name}"), Some(after(a, name.len() + 3))),
                None if spec.long_valued.contains(&t) => (t.to_string(), it.next().cloned()),
                None => (t.to_string(), None),
            };
            p.opts.push(opt);
            continue;
        }
        if t.len() > 1 && t.starts_with('-') {
            for (i, ch) in t.char_indices().skip(1) {
                let rest = i + ch.len_utf8();
                let name = format!("-{ch}");
                if spec.attached.contains(ch) {
                    p.opts.push((name, Some(after(a, rest))));
                    break;
                }
                if spec.valued.contains(ch) {
                    let value = if rest < t.len() {
                        Some(after(a, rest))
                    } else {
                        it.next().cloned()
                    };
                    p.opts.push((name, value));
                    break;
                }
                p.opts.push((name, None));
            }
            continue;
        }
        p.operands.push(a.clone());
    }
    p
}

/// 引数の値の先頭の `n` バイト（字面のオプションの名前）を除いたもの。除く範囲に展開があれば、値を決まらないものにする
fn after(a: &Arg, n: usize) -> Arg {
    let mut skip = n;
    let mut pieces = Vec::new();
    for piece in &a.pieces {
        match piece {
            _ if skip == 0 => pieces.push(piece.clone()),
            Piece::Lit(s) if s.len() <= skip => skip -= s.len(),
            Piece::Lit(s) => {
                pieces.push(Piece::Lit(s.get(skip..).unwrap_or_default().to_string()));
                skip = 0;
            }
            Piece::Var(_) | Piece::Other => {
                pieces = vec![Piece::Other];
                break;
            }
        }
    }
    Arg {
        text: a.text.get(n..).unwrap_or_default().to_string(),
        raw: a.raw.clone(),
        pieces,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEM: &str = "~/.claude/projects/-work-repo/memory";

    fn writes_with(src: &str, config: Option<&str>) -> Vec<PathBuf> {
        let home = crate::shell::home();
        memory_writes(&Shell::parse(src), Path::new("/work/repo"), &home, config)
    }

    fn denied(src: &str) -> bool {
        !writes_with(src, None).is_empty()
    }

    #[test]
    fn writes_into_the_memory_directory_are_denied() {
        for src in [
            format!("echo x > {MEM}/MEMORY.md"),
            format!("echo x >> {MEM}/MEMORY.md"),
            format!("cat > {MEM}/a.md <<'EOF'\nx\nEOF"),
            format!("echo x | tee {MEM}/a.md"),
            format!("echo x | tee -a /tmp/log {MEM}/a.md"),
            format!("sed -i 's/a/b/' {MEM}/MEMORY.md"),
            format!("sed -i.bak -e 's/a/b/' {MEM}/MEMORY.md"),
            format!("sed -Ei 's/a/b/' {MEM}/MEMORY.md"),
            format!("sed --in-place 's/a/b/' {MEM}/MEMORY.md"),
            format!("perl -pi -e 's/a/b/' {MEM}/MEMORY.md"),
            format!("perl -i -pe 's/a/b/' {MEM}/MEMORY.md"),
            // -0 と -l は続く数字だけを値に取り、まとめた残りのオプションは読み続ける
            format!("perl -0pi -e 's/a/b/' {MEM}/MEMORY.md"),
            format!("perl -0777pi -e 's/a/b/' {MEM}/MEMORY.md"),
            format!("perl -lpi -e 's/a/b/' {MEM}/MEMORY.md"),
            format!("cp /tmp/a.md {MEM}/a.md"),
            format!("cp /tmp/a.md {MEM}/"),
            format!("cp -t {MEM} /tmp/a.md"),
            format!("mv /tmp/a.md {MEM}/a.md"),
            "mv --target-directory=$HOME/.claude/projects/-work-repo/memory /tmp/a.md".to_string(),
            format!("install -m 644 /tmp/a.md {MEM}/a.md"),
            format!("truncate -s 0 {MEM}/a.md"),
            format!("cd {MEM} && echo x >> MEMORY.md"),
            format!("cd {MEM} && sed -i 's/a/b/' MEMORY.md"),
            "cd ~/.claude/projects/-work-repo && echo x > memory/a.md".to_string(),
            format!("bash -c 'echo x >> {MEM}/MEMORY.md'"),
            "echo x >> $HOME/.claude/projects/-work-repo/memory/MEMORY.md".to_string(),
            "echo x >> \"${HOME}/.claude/projects/-work-repo/memory/MEMORY.md\"".to_string(),
            "d=~/.claude/projects/-work-repo/memory; echo x >> $d/MEMORY.md".to_string(),
            "d=~/.claude/projects/-work-repo/memory; echo x >> \"$d/MEMORY.md\"".to_string(),
            format!("echo x >> {MEM}/../memory/./MEMORY.md"),
            format!("/usr/bin/sed -i 's/a/b/' {MEM}/MEMORY.md"),
        ] {
            assert!(denied(&src), "{src}");
        }
    }

    #[test]
    fn paths_outside_the_memory_directory_are_allowed() {
        for src in [
            "echo x >> ~/.claude/projects/-work-repo/notes.md",
            "echo x >> ~/.claude/projects/-work-repo/a.jsonl",
            "echo x >> /work/repo/memory/MEMORY.md",
            "echo x >> memory/MEMORY.md",
            "echo x >> ~/.claude/memory/MEMORY.md",
            "echo x >> ~/.claude/projects/memory/a.md",
            "cp ~/.claude/projects/-work-repo/memory/a.md /tmp/a.md",
            "mv ~/.claude/projects/-work-repo/memory/a.md /tmp/",
            "sed 's/a/b/' ~/.claude/projects/-work-repo/memory/MEMORY.md > /tmp/out",
            "perl -pe 's/a/b/' ~/.claude/projects/-work-repo/memory/MEMORY.md",
            "install -d ~/.claude/projects/-work-repo/memory",
            "echo x >> ~/.claude/projects/-work-repo/memory/../notes.md",
            "echo x >> $UNKNOWN/MEMORY.md",
            "ssh r995 'echo x >> ~/.claude/projects/-work-repo/memory/MEMORY.md'",
            // クォートした `~` を bash は展開せず、cwd の下の `~` というディレクトリに書く
            "echo x >> \"~/.claude/projects/-work-repo/memory/MEMORY.md\"",
            "tee '~/.claude/projects/-work-repo/memory/a.md'",
            "echo x >> \\~/.claude/projects/-work-repo/memory/a.md",
            // 語の先頭でない `~`（オプションの名前の後）も bash は展開しない
            "cp --target-directory=~/.claude/projects/-work-repo/memory /tmp/a.md",
            "cp -t~/.claude/projects/-work-repo/memory /tmp/a.md",
        ] {
            assert!(!denied(src), "{src}");
        }
    }

    /// 消すことは、Claude Code が読んだ状態と書き換えた内容の食い違いを作らない（次の Edit は無いファイルで失敗する）。
    /// /handoff は済んだ引き継ぎのファイルを消す手順を持ち、Write と Edit では消せない
    #[test]
    fn removing_memory_files_is_allowed() {
        for src in [
            format!("rm {MEM}/a.md"),
            format!("rm -f -- {MEM}/a.md"),
            format!("rm -r {MEM}"),
            format!("cd {MEM} && rm handoff-2026-11-20.md"),
        ] {
            assert!(!denied(&src), "{src}");
        }
    }

    #[test]
    fn read_only_commands_are_allowed() {
        for src in [
            format!("cat {MEM}/MEMORY.md"),
            format!("grep -n x {MEM}/*.md"),
            format!("ls -la {MEM}"),
            format!("~/.claude/skills/retro/scripts/memory-index.sh {MEM}"),
            format!("wc -l < {MEM}/MEMORY.md"),
            format!("cd {MEM} && cat MEMORY.md"),
            format!("cat {MEM}/MEMORY.md 2>/dev/null > /tmp/copy"),
            format!("sed -n 1,5p {MEM}/MEMORY.md"),
            format!("cd {MEM} && sed -n 1p MEMORY.md 2>&1"),
        ] {
            assert!(!denied(&src), "{src}");
        }
    }

    #[test]
    fn a_moved_config_directory_holds_the_memory() {
        let home = crate::shell::home();
        let config = home.join("cfg").display().to_string();
        let config = Some(config.as_str());
        assert!(!writes_with("echo x >> ~/cfg/projects/-p/memory/a.md", config).is_empty());
        assert!(
            !writes_with(
                "echo x >> $CLAUDE_CONFIG_DIR/projects/-p/memory/a.md",
                config
            )
            .is_empty()
        );
        assert!(writes_with(&format!("echo x >> {MEM}/a.md"), config).is_empty());
    }

    #[test]
    fn the_message_names_the_path_and_the_tools_to_use() {
        // check は hook の環境の CLAUDE_CONFIG_DIR を読むので、実行する人の環境によらない message で確かめる
        let paths = writes_with(&format!("sed -i 's/a/b/' {MEM}/MEMORY.md"), None);
        let msg = message(&paths);
        assert!(msg.contains("MEMORY.md"), "{msg}");
        assert!(msg.contains("Read"), "{msg}");
        assert!(msg.contains("Write") && msg.contains("Edit"), "{msg}");
        assert!(msg.contains("ALLOW_MEMORY_BASH_WRITE=<理由>"), "{msg}");
    }
}
