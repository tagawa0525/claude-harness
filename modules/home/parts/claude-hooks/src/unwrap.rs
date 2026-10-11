//! コマンドを包むもの（env、command、exec、nohup、time、nice、timeout、setsid、stdbuf、ionice）をほどき、
//! 中のコマンドと、包みが変える環境変数・ディレクトリを取り出す
//!
//! 包みごとのオプションの読み方は、coreutils（env、nice、timeout、stdbuf、nohup）、util-linux（setsid、ionice）、
//! GNU time の man と、bash の組み込み（command、exec）から、値を取るものと、ほどき方を変えるものだけを持つ。
//! 短縮形のまとまり（`-iu X`）、続けて書いた値（`-uX`、`--unset=X`）、長い形の一意な省略形（`--ign`）を読む

use std::collections::VecDeque;

use crate::shell::Arg;

/// 包みの長い形のオプション: 名前、同じ意味の短縮形（無ければ None）、値を取るか
type Long = (&'static str, Option<char>, bool);

struct Wrapper {
    name: &'static str,
    /// 値を取る短縮形（続けて書いた残りか、次の語が値）
    value_shorts: &'static str,
    longs: &'static [Long],
    /// オプションの後、コマンドの前に置く位置引数の数（timeout の時間）
    positionals: usize,
}

const WRAPPERS: &[Wrapper] = &[
    Wrapper {
        name: "env",
        value_shorts: "uCSa",
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
    },
    Wrapper {
        name: "command",
        value_shorts: "",
        longs: &[],
        positionals: 0,
    },
    Wrapper {
        name: "exec",
        value_shorts: "a",
        longs: &[],
        positionals: 0,
    },
    Wrapper {
        name: "nohup",
        value_shorts: "",
        longs: &[],
        positionals: 0,
    },
    Wrapper {
        name: "time",
        value_shorts: "fo",
        longs: &[
            ("--format", Some('f'), true),
            ("--output", Some('o'), true),
            ("--append", Some('a'), false),
            ("--verbose", Some('v'), false),
            ("--portability", Some('p'), false),
            ("--quiet", Some('q'), false),
        ],
        positionals: 0,
    },
    Wrapper {
        name: "nice",
        value_shorts: "n",
        longs: &[("--adjustment", Some('n'), true)],
        positionals: 0,
    },
    Wrapper {
        name: "timeout",
        value_shorts: "ks",
        longs: &[
            ("--kill-after", Some('k'), true),
            ("--signal", Some('s'), true),
            ("--preserve-status", None, false),
            ("--foreground", None, false),
            ("--verbose", Some('v'), false),
        ],
        positionals: 1,
    },
    Wrapper {
        name: "setsid",
        value_shorts: "",
        longs: &[
            ("--ctty", Some('c'), false),
            ("--fork", Some('f'), false),
            ("--wait", Some('w'), false),
        ],
        positionals: 0,
    },
    Wrapper {
        name: "stdbuf",
        value_shorts: "ioe",
        longs: &[
            ("--input", Some('i'), true),
            ("--output", Some('o'), true),
            ("--error", Some('e'), true),
        ],
        positionals: 0,
    },
    Wrapper {
        name: "ionice",
        value_shorts: "cnpPu",
        longs: &[
            ("--class", Some('c'), true),
            ("--classdata", Some('n'), true),
            ("--pid", Some('p'), true),
            ("--pgid", Some('P'), true),
            ("--uid", Some('u'), true),
            ("--ignore", Some('t'), false),
        ],
        positionals: 0,
    },
];

/// ほどいた中のコマンドと、包みが変えるもの
#[derive(Debug, Default)]
pub struct Unwrapped {
    pub name: String,
    pub args: Vec<Arg>,
    /// 環境変数の変更（外すなら None）
    pub env: Vec<(String, Option<String>)>,
    /// 受け継ぐ環境変数を全て外すか（`env -i`、`exec -c`）
    pub env_cleared: bool,
    /// 中のコマンドを走らせるディレクトリ（`env -C`。前のものからの相対）
    pub dirs: Vec<String>,
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

/// `env -S` の文字列を、env と同じ決まりで語に分ける（coreutils の env の man の「-S/--split-string usage
/// in scripts」）。空白と `\_` で区切り、単引用符の中はそのまま、二重引用符の中は `\"`・`\\`・`\_`（空白）だけを
/// 読む。`\c` より後は捨てる
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
                Some(other) => word.push(other),
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
                Some('t') => {
                    word.push('\t');
                    in_word = true;
                }
                Some(other) => {
                    word.push(other);
                    in_word = true;
                }
                None => {}
            },
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
        let Some(wrapper) = WRAPPERS.iter().find(|w| w.name == wrapper_name) else {
            return out;
        };
        let mut words: VecDeque<Arg> = out.args.iter().cloned().collect();
        let mut step = Unwrapped::default();
        let mut runs = true;
        while let Some(front) = words.front() {
            let t = front.text.clone();
            if t == "--" {
                words.pop_front();
                break;
            }
            if wrapper.name == "env" && t == "-" {
                // `env -` は `env -i`
                words.pop_front();
                step.env_cleared = true;
                continue;
            }
            if wrapper.name == "env" && !t.starts_with('-') {
                if let Some((var, value)) = t.split_once('=') {
                    words.pop_front();
                    step.env.push((var.to_string(), Some(value.to_string())));
                    continue;
                }
                break;
            }
            if !t.starts_with('-') || t == "-" {
                break;
            }
            words.pop_front();
            // (同じ意味の短縮形, 値)
            let mut options: Vec<(Option<char>, Option<String>, String)> = Vec::new();
            if let Some(long) = t.strip_prefix("--") {
                let (name, attached) = match long.split_once('=') {
                    Some((n, v)) => (format!("--{n}"), Some(v.to_string())),
                    None => (t.clone(), None),
                };
                match long_option(wrapper, &name) {
                    Some((canonical, short, true)) => {
                        let value = attached.or_else(|| words.pop_front().map(|a| a.text));
                        options.push((*short, value, canonical.to_string()));
                    }
                    Some((canonical, short, false)) => {
                        options.push((*short, None, canonical.to_string()))
                    }
                    None => options.push((None, attached, name)),
                }
            } else {
                let cluster: Vec<char> = t[1..].chars().collect();
                for (i, ch) in cluster.iter().enumerate() {
                    if wrapper.value_shorts.contains(*ch) {
                        let rest: String = cluster[i + 1..].iter().collect();
                        let value = if rest.is_empty() {
                            words.pop_front().map(|a| a.text)
                        } else {
                            Some(rest)
                        };
                        options.push((Some(*ch), value, String::new()));
                        break;
                    }
                    options.push((Some(*ch), None, String::new()));
                }
            }
            for (short, value, _) in options {
                match (wrapper.name, short) {
                    ("env", Some('i')) => step.env_cleared = true,
                    ("env", Some('u')) => {
                        if let Some(var) = value {
                            step.env.push((var, None));
                        }
                    }
                    ("env", Some('C')) => {
                        if let Some(dir) = value {
                            step.dirs.push(dir);
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
                    ("exec", Some('c')) => step.env_cleared = true,
                    ("ionice", Some('p' | 'P' | 'u')) => runs = false,
                    _ => {}
                }
            }
        }
        if !runs {
            return out;
        }
        for _ in 0..wrapper.positionals {
            words.pop_front();
        }
        let Some(program) = words.pop_front() else {
            return out;
        };
        out.name = program.text;
        out.args = words.into_iter().collect();
        out.env.extend(step.env);
        out.env_cleared |= step.env_cleared;
        out.dirs.extend(step.dirs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
