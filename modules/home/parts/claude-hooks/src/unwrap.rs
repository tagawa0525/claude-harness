//! コマンドを包むもの（env、builtin、command、exec、nohup、time、nice、timeout、setsid、stdbuf、ionice、sudo、xargs、
//! flock、chrt、taskset、watch、script、ssh、eval、bash、sh）をほどき、中のコマンドと、包みが変える環境変数・
//! ディレクトリを取り出す（ADR-0025）
//!
//! 包みごとのオプションの読み方は、coreutils（env、nice、timeout、stdbuf、nohup）、util-linux（setsid、ionice、flock、
//! chrt、taskset、script）、GNU time、sudo、findutils（xargs）、procps（watch）、OpenSSH（ssh）の man と、bash の
//! 組み込み（builtin、command、exec、eval、source）と bash 自身の起動オプションから、値を取るものと、ほどき方を変える
//! ものだけを持つ。短縮形のまとまり（`-iu X`）、続けて書いた値（`-uX`、`--unset=X`）、長い形の一意な省略形（`--ign`）を読む
//!
//! 包みの中には、次の語をプログラムとして走らせるものと、文字列を sh -c で走らせるもの（`bash -c`、`flock … -c`、
//! `script -c`、`eval`、`watch`、`ssh`。[`Unwrapped::source`]）と、何を走らせるか文字列からは分からないもの
//! （`bash -s`、`source`。[`Unwrapped::opaque`]）がある

use std::collections::VecDeque;

use crate::shell::Arg;

/// 包みの長い形のオプション: 名前、同じ意味の短縮形（無ければ None）、値を取るか
type Long = (&'static str, Option<char>, bool);

struct Wrapper {
    name: &'static str,
    /// 値を取る短縮形（続けて書いた残りか、次の語が値）
    value_shorts: &'static str,
    /// 続けて書いたときだけ値を取る短縮形（getopt の optional_argument。`-i{}` は値つき、`-i` は無し）
    optional_shorts: &'static str,
    longs: &'static [Long],
    /// オプションの後、コマンドの前に置く位置引数の数（timeout の時間、flock のファイル、ssh の接続先）
    positionals: usize,
    /// `NAME=value` を環境変数の代入として読む（env、sudo）
    assigns: bool,
}

/// bash と sh（dash も同じ字）の起動オプション。1 文字のものは set と同じ（`-e`、`-x`、`-o opt`、`+o opt`）
const SHELL_LONGS: &[Long] = &[
    ("--debugger", None, false),
    ("--dump-po-strings", Some('D'), false),
    ("--dump-strings", Some('D'), false),
    ("--help", None, false),
    ("--init-file", None, true),
    ("--login", Some('l'), false),
    ("--noediting", None, false),
    ("--noprofile", None, false),
    ("--norc", None, false),
    ("--posix", None, false),
    ("--pretty-print", None, false),
    ("--rcfile", None, true),
    ("--restricted", Some('r'), false),
    ("--verbose", Some('v'), false),
    ("--version", None, false),
];

const WRAPPERS: &[Wrapper] = &[
    Wrapper {
        name: "env",
        value_shorts: "uCSa",
        optional_shorts: "",
        longs: &[
            ("--ignore-environment", Some('i'), false),
            ("--argv0", Some('a'), true),
            ("--unset", Some('u'), true),
            ("--chdir", Some('C'), true),
            ("--split-string", Some('S'), true),
            ("--null", Some('0'), false),
            ("--debug", Some('v'), false),
            ("--block-signal", None, false),
            ("--default-signal", None, false),
            ("--ignore-signal", None, false),
            ("--list-signal-handling", None, false),
        ],
        positionals: 0,
        assigns: true,
    },
    Wrapper {
        name: "builtin",
        value_shorts: "",
        optional_shorts: "",
        longs: &[],
        positionals: 0,
        assigns: false,
    },
    Wrapper {
        name: "command",
        value_shorts: "",
        optional_shorts: "",
        longs: &[],
        positionals: 0,
        assigns: false,
    },
    Wrapper {
        name: "exec",
        value_shorts: "a",
        optional_shorts: "",
        longs: &[],
        positionals: 0,
        assigns: false,
    },
    Wrapper {
        name: "eval",
        value_shorts: "",
        optional_shorts: "",
        longs: &[],
        positionals: 0,
        assigns: false,
    },
    Wrapper {
        name: "nohup",
        value_shorts: "",
        optional_shorts: "",
        longs: &[],
        positionals: 0,
        assigns: false,
    },
    Wrapper {
        name: "time",
        value_shorts: "fo",
        optional_shorts: "",
        longs: &[
            ("--format", Some('f'), true),
            ("--output", Some('o'), true),
            ("--append", Some('a'), false),
            ("--verbose", Some('v'), false),
            ("--portability", Some('p'), false),
            ("--quiet", Some('q'), false),
        ],
        positionals: 0,
        assigns: false,
    },
    Wrapper {
        name: "nice",
        value_shorts: "n",
        optional_shorts: "",
        longs: &[("--adjustment", Some('n'), true)],
        positionals: 0,
        assigns: false,
    },
    Wrapper {
        name: "timeout",
        value_shorts: "ks",
        optional_shorts: "",
        longs: &[
            ("--kill-after", Some('k'), true),
            ("--signal", Some('s'), true),
            ("--preserve-status", None, false),
            ("--foreground", None, false),
            ("--verbose", Some('v'), false),
        ],
        positionals: 1,
        assigns: false,
    },
    Wrapper {
        name: "setsid",
        value_shorts: "",
        optional_shorts: "",
        longs: &[
            ("--ctty", Some('c'), false),
            ("--fork", Some('f'), false),
            ("--wait", Some('w'), false),
        ],
        positionals: 0,
        assigns: false,
    },
    Wrapper {
        name: "stdbuf",
        value_shorts: "ioe",
        optional_shorts: "",
        longs: &[
            ("--input", Some('i'), true),
            ("--output", Some('o'), true),
            ("--error", Some('e'), true),
        ],
        positionals: 0,
        assigns: false,
    },
    Wrapper {
        name: "ionice",
        value_shorts: "cnpPu",
        optional_shorts: "",
        longs: &[
            ("--class", Some('c'), true),
            ("--classdata", Some('n'), true),
            ("--pid", Some('p'), true),
            ("--pgid", Some('P'), true),
            ("--uid", Some('u'), true),
            ("--ignore", Some('t'), false),
        ],
        positionals: 0,
        assigns: false,
    },
    Wrapper {
        name: "sudo",
        value_shorts: "aCDghpRrTtUu",
        optional_shorts: "",
        longs: &[
            ("--askpass", Some('A'), false),
            ("--auth-type", Some('a'), true),
            ("--bell", Some('B'), false),
            ("--background", Some('b'), false),
            ("--close-from", Some('C'), true),
            ("--chdir", Some('D'), true),
            ("--preserve-env", Some('E'), false),
            ("--edit", Some('e'), false),
            ("--group", Some('g'), true),
            ("--set-home", Some('H'), false),
            ("--help", None, false),
            ("--host", Some('h'), true),
            ("--login", Some('i'), false),
            ("--remove-timestamp", Some('K'), false),
            ("--reset-timestamp", Some('k'), false),
            ("--list", Some('l'), false),
            ("--no-update", Some('N'), false),
            ("--non-interactive", Some('n'), false),
            ("--preserve-groups", Some('P'), false),
            ("--prompt", Some('p'), true),
            ("--chroot", Some('R'), true),
            ("--role", Some('r'), true),
            ("--stdin", Some('S'), false),
            ("--shell", Some('s'), false),
            ("--command-timeout", Some('T'), true),
            ("--type", Some('t'), true),
            ("--other-user", Some('U'), true),
            ("--user", Some('u'), true),
            ("--version", Some('V'), false),
            ("--validate", Some('v'), false),
        ],
        positionals: 0,
        assigns: true,
    },
    Wrapper {
        name: "xargs",
        value_shorts: "adEILnPs",
        optional_shorts: "eil",
        longs: &[
            ("--null", Some('0'), false),
            ("--arg-file", Some('a'), true),
            ("--delimiter", Some('d'), true),
            ("--eof", Some('e'), false),
            ("--replace", Some('i'), false),
            ("--max-lines", Some('l'), false),
            ("--max-args", Some('n'), true),
            ("--max-procs", Some('P'), true),
            ("--open-tty", Some('o'), false),
            ("--interactive", Some('p'), false),
            ("--process-slot-var", None, true),
            ("--no-run-if-empty", Some('r'), false),
            ("--max-chars", Some('s'), true),
            ("--show-limits", None, false),
            ("--verbose", Some('t'), false),
            ("--exit", Some('x'), false),
            ("--help", None, false),
            ("--version", None, false),
        ],
        positionals: 0,
        assigns: false,
    },
    Wrapper {
        name: "flock",
        value_shorts: "Ew",
        optional_shorts: "",
        longs: &[
            ("--conflict-exit-code", Some('E'), true),
            ("--no-fork", Some('F'), false),
            ("--exclusive", Some('x'), false),
            ("--nb", Some('n'), false),
            ("--nonblock", Some('n'), false),
            ("--close", Some('o'), false),
            ("--shared", Some('s'), false),
            ("--unlock", Some('u'), false),
            ("--wait", Some('w'), true),
            ("--timeout", Some('w'), true),
            ("--fcntl", None, false),
            ("--start", None, true),
            ("--length", None, true),
            ("--verbose", None, false),
            ("--help", Some('h'), false),
            ("--version", Some('V'), false),
        ],
        positionals: 1,
        assigns: false,
    },
    Wrapper {
        name: "chrt",
        value_shorts: "TPD",
        optional_shorts: "",
        longs: &[
            ("--other", Some('o'), false),
            ("--fifo", Some('f'), false),
            ("--rr", Some('r'), false),
            ("--batch", Some('b'), false),
            ("--idle", Some('i'), false),
            ("--deadline", Some('d'), false),
            ("--ext", Some('e'), false),
            ("--sched-runtime", Some('T'), true),
            ("--sched-period", Some('P'), true),
            ("--sched-deadline", Some('D'), true),
            ("--reset-on-fork", Some('R'), false),
            ("--all-tasks", Some('a'), false),
            ("--max", Some('m'), false),
            ("--pid", Some('p'), false),
            ("--verbose", Some('v'), false),
            ("--help", Some('h'), false),
            ("--version", Some('V'), false),
        ],
        positionals: 1,
        assigns: false,
    },
    Wrapper {
        name: "taskset",
        value_shorts: "",
        optional_shorts: "",
        longs: &[
            ("--all-tasks", Some('a'), false),
            ("--cpu-list", Some('c'), false),
            ("--pid", Some('p'), false),
            ("--help", Some('h'), false),
            ("--version", Some('V'), false),
        ],
        positionals: 1,
        assigns: false,
    },
    Wrapper {
        name: "watch",
        value_shorts: "nqs",
        optional_shorts: "d",
        longs: &[
            ("--beep", Some('b'), false),
            ("--color", Some('c'), false),
            ("--no-color", Some('C'), false),
            ("--differences", Some('d'), false),
            ("--errexit", Some('e'), false),
            ("--follow", Some('f'), false),
            ("--chgexit", Some('g'), false),
            ("--interval", Some('n'), true),
            ("--precise", Some('p'), false),
            ("--equexit", Some('q'), true),
            ("--no-rerun", Some('r'), false),
            ("--shotsdir", Some('s'), true),
            ("--no-title", Some('t'), false),
            ("--no-wrap", Some('w'), false),
            ("--exec", Some('x'), false),
            ("--help", Some('h'), false),
            ("--version", Some('v'), false),
        ],
        positionals: 0,
        assigns: false,
    },
    Wrapper {
        name: "script",
        value_shorts: "cEIOBTmo",
        optional_shorts: "t",
        longs: &[
            ("--append", Some('a'), false),
            ("--command", Some('c'), true),
            ("--echo", Some('E'), true),
            ("--return", Some('e'), false),
            ("--flush", Some('f'), false),
            ("--force", None, false),
            ("--log-io", Some('B'), true),
            ("--log-in", Some('I'), true),
            ("--log-out", Some('O'), true),
            ("--log-timing", Some('T'), true),
            ("--logging-format", Some('m'), true),
            ("--output-limit", Some('o'), true),
            ("--quiet", Some('q'), false),
            ("--timing", Some('t'), false),
            ("--help", Some('h'), false),
            ("--version", Some('V'), false),
        ],
        positionals: 0,
        assigns: false,
    },
    Wrapper {
        name: "ssh",
        value_shorts: "BbcDEeFIiJLlmOoPpQRSWw",
        optional_shorts: "",
        longs: &[],
        positionals: 1,
        assigns: false,
    },
    Wrapper {
        name: "bash",
        value_shorts: "oO",
        optional_shorts: "",
        longs: SHELL_LONGS,
        positionals: 0,
        assigns: false,
    },
    Wrapper {
        name: "sh",
        value_shorts: "oO",
        optional_shorts: "",
        longs: SHELL_LONGS,
        positionals: 0,
        assigns: false,
    },
];

/// 環境変数の変更 1 つ。順に効く（`env -i CLAUDECODE=1` は外した後で戻す）
#[derive(Debug, Clone, PartialEq)]
pub enum EnvChange {
    Set(String, String),
    Unset(String),
    /// 受け継ぐ環境変数を全て外す（`env -i`、`exec -c`、`-E` の無い `sudo`、`ssh` の接続先）
    Clear,
}

/// 変更を順に当てた後の、環境変数 `name` の値（`inherited` は受け継いだ値）
pub fn env_value<'a>(
    changes: &'a [EnvChange],
    name: &str,
    inherited: Option<&'a str>,
) -> Option<&'a str> {
    changes
        .iter()
        .fold(inherited, |value, change| match change {
            EnvChange::Set(n, v) if n == name => Some(v),
            EnvChange::Unset(n) if n == name => None,
            EnvChange::Clear => None,
            _ => value,
        })
}

/// ほどいた中のコマンドと、包みが変えるもの
#[derive(Debug, Default)]
pub struct Unwrapped {
    pub name: String,
    pub args: Vec<Arg>,
    /// 環境変数の変更（順に効く）
    pub env: Vec<EnvChange>,
    /// ほどいた包みの、書いたままの名前（外側から順に。`/usr/bin/time` はパスのまま）
    pub wrappers: Vec<String>,
    /// 中のコマンドを走らせるディレクトリ（`env -C`、`sudo -D`。前のものからの相対）
    pub dirs: Vec<String>,
    /// `name` が sh -c で走らせる文字列（`bash -c '…'`、`flock <file> -c '…'`、`script -c '…'`、`eval`・`watch`・
    /// `ssh <接続先>` の残りの語を空白でつないだもの）。shell.rs が再帰的に解析する
    pub source: Option<String>,
    /// `name` が何を走らせるか、文字列からは分からない: 標準入力からコマンドを読む（`bash -s`、引数の無い `bash`、
    /// コマンドの無い `ssh <接続先>` / `sudo -s` / `script`）、ファイルを読んで走らせる（`source` / `.`）
    pub opaque: bool,
}

pub fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// 長い形の名前（`--x` か一意な省略形）を解決する
fn long_option(wrapper: &Wrapper, name: &str) -> Option<&'static Long> {
    if let Some(exact) = wrapper.longs.iter().find(|(long, _, _)| *long == name) {
        return Some(exact);
    }
    let mut matches = wrapper
        .longs
        .iter()
        .filter(|(long, _, _)| long.starts_with(name));
    match (matches.next(), matches.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}

fn arg(text: &str) -> Arg {
    Arg {
        text: text.to_string(),
        raw: text.to_string(),
    }
}

/// `env -S` の `\n`・`\t`・`\r`・`\f`・`\v` が表す制御文字
fn control(c: char) -> Option<char> {
    match c {
        'n' => Some('\n'),
        't' => Some('\t'),
        'r' => Some('\r'),
        'f' => Some('\x0c'),
        'v' => Some('\x0b'),
        _ => None,
    }
}

/// `env -S` の文字列を、env と同じ決まりで語に分ける（coreutils の env の man の「-S/--split-string usage
/// in scripts」）。空白と `\_` で区切り、単引用符の中はそのまま、二重引用符の中はエスケープ（`\"`・`\\`・
/// `\_`（空白）・制御文字）だけを読む。`\c` より後と、語の先頭の `#` より後は捨てる
pub fn split_env_string(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = s.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('\''), _) => word.push(c),
            (Some(_), '\\') => match chars.next() {
                Some('_') => word.push(' '),
                Some(other) => word.push(control(other).unwrap_or(other)),
                None => {}
            },
            (Some(_), _) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                in_word = true;
            }
            (None, '\\') => match chars.next() {
                Some('_') => {
                    if in_word {
                        words.push(std::mem::take(&mut word));
                        in_word = false;
                    }
                }
                Some('c') => break,
                Some(other) => {
                    word.push(control(other).unwrap_or(other));
                    in_word = true;
                }
                None => {}
            },
            // 語の先頭の # から後ろは注釈
            (None, '#') if !in_word => break,
            (None, c) if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            (None, c) => {
                word.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push(word);
    }
    words
}

/// 残りの語を空白でつないだ、sh -c で走らせる文字列（eval、watch、ssh は語をつないで渡す）
fn joined(words: &VecDeque<Arg>) -> String {
    words
        .iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `name args…` が包みなら、ほどいた中のコマンドを返す。`command -v` / `-V`（`-pv` などのまとまりを含む）は
/// プログラムを探して表示するだけで走らせず、`ionice -p` などは既にあるプロセスを対象にするので、ほどかない
pub fn unwrap(name: &str, args: &[Arg]) -> Unwrapped {
    let mut out = Unwrapped {
        name: name.to_string(),
        args: args.to_vec(),
        ..Unwrapped::default()
    };
    loop {
        let wrapper_name = basename(&out.name).to_string();
        // source / . はファイルを読んで走らせるので、何を走らせるかは文字列からは分からない
        if matches!(wrapper_name.as_str(), "source" | ".") {
            out.opaque = true;
            return out;
        }
        let Some(wrapper) = WRAPPERS.iter().find(|w| w.name == wrapper_name) else {
            return out;
        };
        let is_shell = matches!(wrapper.name, "bash" | "sh");
        let mut words: VecDeque<Arg> = out.args.iter().cloned().collect();
        // この包み 1 段の環境の変更と、移るディレクトリ（最後の -C / -D が効く）。out に直に書かないのは、
        // 包みが中のコマンドを走らせない（--help など）ときに捨てるため
        let mut env: Vec<EnvChange> = Vec::new();
        let mut dir: Option<String> = None;
        let mut runs = true;
        let mut options_done = false;
        // bash -c: 次の位置引数を sh -c の文字列として走らせる
        let mut takes_source = false;
        // bash -s、sudo -s / -i: 標準入力からコマンドを読む（sudo はコマンドが無ければ対話シェル）
        let mut reads_stdin = false;
        // watch -x: 文字列を sh -c に渡さず、語をそのまま exec する
        let mut execs = false;
        // sudo は -E が無ければ環境変数を受け継がない（env_reset）。ssh の接続先には環境変数は渡らない
        let mut preserves_env = !matches!(wrapper.name, "sudo" | "ssh");
        // flock <file> -c、script -c: 走らせる文字列
        let mut source: Option<String> = None;
        // コマンドの前の位置引数のうち、まだ読んでいない数
        let mut positionals = wrapper.positionals;
        // script のログのファイルを読んだか（getopt が語を並べ替えるので、ファイルの後ろのオプションも読む）
        let mut log_file_seen = false;
        loop {
            while let Some(front) = words.front() {
                let t = front.text.clone();
                // bash は `-` も `--` と同じにオプションの終わりと読む
                if t == "--" || (is_shell && t == "-") {
                    words.pop_front();
                    options_done = true;
                    continue;
                }
                if wrapper.name == "env" && t == "-" && !options_done {
                    // `env -` は `env -i`
                    words.pop_front();
                    env.push(EnvChange::Clear);
                    continue;
                }
                // env と sudo は `--` の後も `NAME=value` を代入として読む。最初の代入でオプションは終わる
                // （getopt の `+`。`env FOO=1 -i x` は -i というプログラムを走らせる）
                if wrapper.assigns && (options_done || !t.starts_with('-')) {
                    if let Some((var, value)) = t.split_once('=') {
                        words.pop_front();
                        env.push(EnvChange::Set(var.to_string(), value.to_string()));
                        options_done = true;
                        continue;
                    }
                    break;
                }
                // bash の `+o opt` / `+O shopt` は set と同じで、`-o` / `-O` と同じく値を取る
                let plus = is_shell && t.starts_with('+') && t.len() > 1;
                if options_done || !(t.starts_with('-') || plus) || t == "-" {
                    break;
                }
                words.pop_front();
                // (同じ意味の短縮形, 値)
                let mut options: Vec<(Option<char>, Option<String>)> = Vec::new();
                if let Some(long) = t.strip_prefix("--") {
                    let (name, attached) = match long.split_once('=') {
                        Some((n, v)) => (format!("--{n}"), Some(v.to_string())),
                        None => (t.clone(), None),
                    };
                    // どの包みも --help / --version は表示して終わり、後ろの語を走らせない
                    if name == "--help" || name == "--version" {
                        runs = false;
                    }
                    match long_option(wrapper, &name) {
                        Some((_, short, true)) => {
                            let value = attached.or_else(|| words.pop_front().map(|a| a.text));
                            options.push((*short, value));
                        }
                        // 値を取らない長い形でも、`--preserve-env=list` のように `=` で付けた値は残す
                        Some((_, short, false)) => options.push((*short, attached)),
                        None => options.push((None, attached)),
                    }
                } else {
                    let cluster: Vec<char> = t[1..].chars().collect();
                    for (i, ch) in cluster.iter().enumerate() {
                        let rest: String = cluster[i + 1..].iter().collect();
                        // bash と sh は -o / -O の値を常に次の語から取り、まとまりの残りの文字もオプションとして
                        // 読む（`bash -oc pipefail '…'` は -o pipefail と -c）。getopt のように残りを値にしない
                        if is_shell && wrapper.value_shorts.contains(*ch) {
                            options.push((Some(*ch), words.pop_front().map(|a| a.text)));
                            continue;
                        }
                        if wrapper.value_shorts.contains(*ch) {
                            let value = if rest.is_empty() {
                                words.pop_front().map(|a| a.text)
                            } else {
                                Some(rest)
                            };
                            options.push((Some(*ch), value));
                            break;
                        }
                        if wrapper.optional_shorts.contains(*ch) {
                            options.push((Some(*ch), (!rest.is_empty()).then_some(rest)));
                            break;
                        }
                        options.push((Some(*ch), None));
                    }
                }
                for (short, value) in options {
                    match (wrapper.name, short) {
                        ("env", Some('i')) => env.push(EnvChange::Clear),
                        ("env", Some('u')) => {
                            if let Some(var) = value {
                                env.push(EnvChange::Unset(var));
                            }
                        }
                        ("env", Some('C')) => {
                            if let Some(d) = value {
                                // 1 回の env では最後の -C だけが効く（coreutils の env.c は上書きする）
                                dir = Some(d);
                            }
                        }
                        ("env", Some('S')) => {
                            if let Some(split) = value {
                                for part in split_env_string(&split).iter().rev() {
                                    words.push_front(arg(part));
                                }
                            }
                        }
                        ("command", Some('v' | 'V')) => runs = false,
                        ("exec", Some('c')) => env.push(EnvChange::Clear),
                        ("ionice", Some('p' | 'P' | 'u')) => runs = false,
                        ("bash" | "sh", Some('c')) => takes_source = true,
                        ("bash" | "sh", Some('s')) => reads_stdin = true,
                        // -n は構文を調べるだけ、-D は文字列を列挙するだけで走らせない
                        ("bash" | "sh", Some('n' | 'D')) => runs = false,
                        // `--preserve-env=list` は list の変数だけを受け継ぐ（list は見ない）
                        ("sudo", Some('E')) => preserves_env = value.is_none(),
                        ("sudo", Some('e' | 'l' | 'v' | 'V' | 'K')) => runs = false,
                        ("sudo", Some('s' | 'i')) => reads_stdin = true,
                        ("sudo", Some('D')) => {
                            if let Some(d) = value {
                                dir = Some(d);
                            }
                        }
                        ("chrt", Some('p' | 'm')) | ("taskset", Some('p')) => runs = false,
                        ("watch", Some('x')) => execs = true,
                        ("script", Some('c')) => source = value,
                        ("ssh", Some('G' | 'N' | 'O' | 'Q' | 'V' | 'Z' | 's')) => runs = false,
                        _ => {}
                    }
                }
            }
            if !runs {
                return out;
            }
            if positionals > 0 {
                words.pop_front();
                positionals -= 1;
                // ssh は接続先を取った後、オプションの読み取りに戻る（ssh.c は host を取ると getopt をやり直す）
                if wrapper.name == "ssh" && !options_done {
                    continue;
                }
            } else if wrapper.name == "script"
                && !options_done
                && !log_file_seen
                && !words.is_empty()
            {
                words.pop_front();
                log_file_seen = true;
                continue;
            }
            break;
        }
        if !preserves_env {
            out.env.push(EnvChange::Clear);
        }
        out.env.extend(env);
        out.dirs.extend(dir);
        // 残りの語から、走らせるものを決める
        match wrapper.name {
            "bash" | "sh" => {
                if takes_source {
                    // 文字列の後ろの語は $0 以降
                    source = words.pop_front().map(|w| w.text);
                } else if reads_stdin || words.is_empty() {
                    out.opaque = true;
                    return out;
                }
            }
            "sudo" => {
                if reads_stdin && words.is_empty() {
                    out.opaque = true;
                    return out;
                }
            }
            // ファイルの直後の `-c <文字列>` を sh -c で走らせる（flock.c は getopt の後に argv[optind + 1] を比べる）
            "flock" => {
                if words
                    .front()
                    .is_some_and(|w| w.text == "-c" || w.text == "--command")
                {
                    words.pop_front();
                    source = words.pop_front().map(|w| w.text);
                }
            }
            // -c が無ければ、`--` の後ろの語をコマンドとして走らせる。それも無ければ対話シェル
            "script" if source.is_none() => {
                if !options_done {
                    out.opaque = true;
                    return out;
                }
            }
            // 残りの語を空白でつないで sh -c で走らせる（ssh は接続先のシェルで。コマンドが無ければ標準入力を読む）
            "eval" | "watch" | "ssh" if !execs => {
                if words.is_empty() {
                    if wrapper.name == "ssh" {
                        out.opaque = true;
                    }
                    return out;
                }
                source = Some(joined(&words));
            }
            _ => {}
        }
        if source.is_some() {
            out.source = source;
            return out;
        }
        let Some(program) = words.pop_front() else {
            return out;
        };
        out.wrappers.push(std::mem::take(&mut out.name));
        out.name = program.text;
        out.args = words.into_iter().collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 空白で分けた語をほどく（クォートの要る形は shell.rs のテストで）
    fn un(src: &str) -> Unwrapped {
        let words: Vec<&str> = src.split_whitespace().collect();
        let args: Vec<Arg> = words[1..].iter().map(|w| arg(w)).collect();
        unwrap(words[0], &args)
    }

    #[test]
    fn unwraps_sudo_xargs_flock_chrt_taskset_watch_script_and_bash() {
        for src in [
            "sudo sbatch --wrap x",
            "sudo -u slurm -g users -H sbatch",
            "sudo -E FOO=1 -- sbatch",
            "sudo --user=slurm -n -D /tmp sbatch",
            "sudo -s sbatch",
            "xargs sbatch",
            "xargs -0 -I{} -n1 -P 4 sbatch",
            "xargs -I {} -t sbatch",
            "xargs -i sbatch",
            "xargs -l2 -e -r sbatch",
            "xargs --max-args=1 --process-slot-var=N sbatch",
            "flock /tmp/l sbatch",
            "flock -w 10 -x /tmp/l sbatch",
            "flock --timeout=10 -E 3 /tmp/l sbatch",
            "chrt -f 10 sbatch",
            "chrt --idle 0 sbatch",
            "chrt -T 1000 -d 0 sbatch",
            "taskset 0x1 sbatch",
            "taskset -c 0-3 sbatch",
            "taskset --cpu-list 0 sbatch",
            "watch -x sbatch",
            "watch -n 5 -x sbatch",
            "script log.txt -- sbatch",
            "bash -x sbatch",
            "sh -eu -o pipefail sbatch",
            "bash --rcfile r -- sbatch",
            "sudo flock /tmp/l chrt -f 10 sbatch",
            "bash - sbatch",
            "script log.txt -q -- sbatch",
        ] {
            let u = un(src);
            assert_eq!(u.name, "sbatch", "{src}");
            assert!(u.source.is_none() && !u.opaque, "{src}");
        }
        // env と sudo は、最初の代入でオプションが終わる（getopt の `+`）。後ろの -i は走らせるプログラム
        let u = un("env FOO=1 -i x");
        assert_eq!(u.name, "-i");
        assert_eq!(u.env, [EnvChange::Set("FOO".to_string(), "1".to_string())]);
        assert_eq!(un("sudo FOO=1 -u root x").name, "-u");
        // sudo -D はディレクトリを変える
        assert_eq!(un("sudo -D /tmp git commit").dirs, ["/tmp"]);
        assert_eq!(un("sudo --chdir=/tmp git commit").dirs, ["/tmp"]);
        assert_eq!(un("sudo -u slurm sbatch --wrap x").args[1].text, "x");
        assert_eq!(
            un("sudo flock /tmp/l chrt -f 10 sbatch").wrappers,
            ["sudo", "flock", "chrt"]
        );
    }

    #[test]
    fn wrappers_that_run_a_shell_string_give_the_source() {
        for (src, source) in [
            ("watch -n 5 sbatch --wrap x", "sbatch --wrap x"),
            ("watch -dpermanent sbatch", "sbatch"),
            ("watch --interval=5 -d sbatch", "sbatch"),
            ("ssh r995 sbatch --wrap x", "sbatch --wrap x"),
            ("ssh -p 22 -o X=y -i k -J b user@r995 sbatch", "sbatch"),
            ("ssh -t r995 sbatch", "sbatch"),
            ("flock /tmp/l -c sbatch", "sbatch"),
            ("flock -n /tmp/l --command sbatch", "sbatch"),
            ("script -c sbatch log.txt", "sbatch"),
            ("script -q --command sbatch", "sbatch"),
            ("eval sbatch --wrap x", "sbatch --wrap x"),
            ("bash -c sbatch arg0", "sbatch"),
            ("sh -ec sbatch", "sbatch"),
            ("bash -o pipefail -c sbatch", "sbatch"),
            ("nohup bash -c sbatch", "sbatch"),
            // ssh は接続先の後ろのオプションも読む。script の getopt は語を並べ替える
            ("ssh r995 -t sbatch --wrap x", "sbatch --wrap x"),
            ("ssh r995 -p 22 -- sbatch", "sbatch"),
            ("script log.txt -c sbatch", "sbatch"),
            ("script log.txt -q --command sbatch", "sbatch"),
            ("ssh r995 bash -c sbatch", "bash -c sbatch"),
        ] {
            let u = un(src);
            assert_eq!(u.source.as_deref(), Some(source), "{src}");
            assert!(!u.opaque, "{src}");
        }
    }

    #[test]
    fn shell_option_values_in_a_cluster_are_the_next_words() {
        // bash と sh は、まとめた 1 文字のオプションの中の -o / -O の値を次の語から取り、残りの文字もオプションとして読む
        for (src, source) in [
            ("bash -oc pipefail sbatch", "sbatch"),
            ("bash -Oc extglob sbatch", "sbatch"),
            ("sh -oec pipefail sbatch", "sbatch"),
            ("bash -co pipefail sbatch", "sbatch"),
            ("bash -oOc pipefail extglob sbatch", "sbatch"),
        ] {
            let u = un(src);
            assert_eq!(u.source.as_deref(), Some(source), "{src}");
            assert!(!u.opaque, "{src}");
        }
        let u = un("bash -ox pipefail sbatch");
        assert_eq!(u.name, "sbatch");
        assert!(u.source.is_none() && !u.opaque);
    }

    #[test]
    fn env_reads_a_single_dash_once_after_the_options() {
        // `-` は getopt の後に一度だけ（`--` の後でも）見て、`-i` と同じに読み、そこでオプションを終える
        let u = un("env -- - git push");
        assert_eq!(u.name, "git");
        assert_eq!(u.env, [EnvChange::Clear]);
        let u = un("env - -u FOO git");
        assert_eq!(u.name, "-u");
        assert_eq!(u.env, [EnvChange::Clear]);
        let u = un("env - - cmd");
        assert_eq!(u.name, "-");
        assert_eq!(u.env, [EnvChange::Clear]);
        let u = un("env -u FOO - git");
        assert_eq!(u.name, "git");
        assert_eq!(
            u.env,
            [EnvChange::Unset("FOO".to_string()), EnvChange::Clear]
        );
        let u = un("env - FOO=1 git");
        assert_eq!(u.name, "git");
        assert_eq!(
            u.env,
            [
                EnvChange::Clear,
                EnvChange::Set("FOO".to_string(), "1".to_string())
            ]
        );
    }

    #[test]
    fn wrappers_that_do_not_run_the_command_are_left_alone() {
        for src in [
            "sudo -l sbatch",
            "sudo -e f",
            "sudo -v",
            "sudo -K",
            "chrt -p 10 1234",
            "chrt -m",
            "taskset -p 1 1234",
            "taskset -ap 1234",
            "ssh -N r995",
            "ssh r995 -N",
            "ssh -G r995",
            "ssh -Q cipher",
            "flock 9",
            "flock -u 9",
            "bash -n s.sh",
            "xargs --help",
            "xargs --help sbatch",
            "bash --help sbatch",
            "bash --version sbatch",
            "env --version sbatch",
            "nice --help sbatch",
            "watch --help sbatch",
            "sudo --help sbatch",
            "sudo",
        ] {
            let u = un(src);
            assert_eq!(u.name, src.split(' ').next().unwrap(), "{src}");
            assert!(u.source.is_none() && !u.opaque, "{src}");
        }
    }

    #[test]
    fn shells_reading_stdin_and_sourced_files_are_opaque() {
        for src in [
            "bash -s",
            "bash",
            "sh -s x",
            "bash -s -- x",
            "ssh r995",
            "sudo -s",
            "sudo -i",
            "script log.txt",
            "script",
            "source f",
            ". f",
            "builtin source f",
            "command . f",
        ] {
            assert!(un(src).opaque, "{src}");
        }
    }

    #[test]
    fn sudo_and_ssh_drop_the_environment_unless_preserved() {
        let set = |n: &str, v: &str| EnvChange::Set(n.to_string(), v.to_string());
        assert_eq!(un("sudo git commit").env, [EnvChange::Clear]);
        assert_eq!(un("sudo -i git commit").env, [EnvChange::Clear]);
        assert_eq!(un("sudo -E git commit").env, []);
        assert_eq!(un("sudo --preserve-env git commit").env, []);
        assert_eq!(
            un("sudo --preserve-env=HOME git commit").env,
            [EnvChange::Clear]
        );
        assert_eq!(
            un("sudo FOO=1 git commit").env,
            [EnvChange::Clear, set("FOO", "1")]
        );
        assert_eq!(un("sudo -E FOO=1 git commit").env, [set("FOO", "1")]);
        assert_eq!(un("ssh host git commit").env, [EnvChange::Clear]);
        assert_eq!(un("watch git commit").env, []);
    }

    #[test]
    fn splits_env_strings_like_env() {
        assert_eq!(
            split_env_string("git\\_commit\\_--no-verify"),
            ["git", "commit", "--no-verify"]
        );
        assert_eq!(
            split_env_string("git commit -m 'x -n'"),
            ["git", "commit", "-m", "x -n"]
        );
        assert_eq!(split_env_string("a \"b\\_c\" d\\c e"), ["a", "b c", "d"]);
        // 語の先頭の # から後ろは注釈。語の途中と引用符の中の # は文字
        assert_eq!(split_env_string("a # b c"), ["a"]);
        assert!(split_env_string("# x").is_empty());
        assert_eq!(split_env_string("a#b '#c'"), ["a#b", "#c"]);
        // \n \t \r \f \v は引用符の外と二重引用符の中で制御文字になる。単引用符の中はそのまま
        assert_eq!(
            split_env_string("\"a\\nb\" c\\nd e\\tf \"g\\rh\\fi\\vj\" 'k\\nl'"),
            ["a\nb", "c\nd", "e\tf", "g\rh\x0ci\x0bj", "k\\nl"]
        );
    }
}
