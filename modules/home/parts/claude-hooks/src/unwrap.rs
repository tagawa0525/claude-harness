//! コマンドを包むもの（env、builtin、command、exec、nohup、time、nice、timeout、setsid、stdbuf、ionice）をほどき、
//! 中のコマンドと、包みが変える環境変数・ディレクトリを取り出す
//!
//! 包みごとのオプションの読み方は、coreutils（env、nice、timeout、stdbuf、nohup）、util-linux（setsid、ionice）、
//! GNU time の man と、bash の組み込み（builtin、command、exec）から、値を取るものと、ほどき方を変えるものだけを持つ。
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
        name: "builtin",
        value_shorts: "",
        longs: &[],
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

/// 環境変数の変更 1 つ。順に効く（`env -i CLAUDECODE=1` は外した後で戻す）
#[derive(Debug, Clone, PartialEq)]
pub enum EnvChange {
    Set(String, String),
    Unset(String),
    /// 受け継ぐ環境変数を全て外す（`env -i`、`exec -c`）
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
        let mut options_done = false;
        while let Some(front) = words.front() {
            let t = front.text.clone();
            if t == "--" {
                words.pop_front();
                options_done = true;
                continue;
            }
            if wrapper.name == "env" && t == "-" && !options_done {
                // `env -` は `env -i`
                words.pop_front();
                step.env.push(EnvChange::Clear);
                continue;
            }
            // env は `--` の後も `NAME=value` を代入として読む
            if wrapper.name == "env" && (options_done || !t.starts_with('-')) {
                if let Some((var, value)) = t.split_once('=') {
                    words.pop_front();
                    step.env
                        .push(EnvChange::Set(var.to_string(), value.to_string()));
                    continue;
                }
                break;
            }
            if options_done || !t.starts_with('-') || t == "-" {
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
                match long_option(wrapper, &name) {
                    Some((_, short, true)) => {
                        let value = attached.or_else(|| words.pop_front().map(|a| a.text));
                        options.push((*short, value));
                    }
                    Some((_, short, false)) => options.push((*short, None)),
                    None => options.push((None, attached)),
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
                        options.push((Some(*ch), value));
                        break;
                    }
                    options.push((Some(*ch), None));
                }
            }
            for (short, value) in options {
                match (wrapper.name, short) {
                    ("env", Some('i')) => step.env.push(EnvChange::Clear),
                    ("env", Some('u')) => {
                        if let Some(var) = value {
                            step.env.push(EnvChange::Unset(var));
                        }
                    }
                    ("env", Some('C')) => {
                        if let Some(dir) = value {
                            // 1 回の env では最後の -C だけが効く（coreutils の env.c は上書きする）
                            step.dirs = vec![dir];
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
                    ("exec", Some('c')) => step.env.push(EnvChange::Clear),
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
        out.wrappers.push(std::mem::take(&mut out.name));
        out.name = program.text;
        out.args = words.into_iter().collect();
        out.env.extend(step.env);
        out.dirs.extend(step.dirs);
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
        ] {
            let u = un(src);
            assert_eq!(u.name, "sbatch", "{src}");
            assert!(u.source.is_none() && !u.opaque, "{src}");
        }
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
            ("ssh r995 bash -c sbatch", "bash -c sbatch"),
        ] {
            let u = un(src);
            assert_eq!(u.source.as_deref(), Some(source), "{src}");
            assert!(!u.opaque, "{src}");
        }
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
            "ssh -G r995",
            "ssh -Q cipher",
            "flock 9",
            "flock -u 9",
            "bash -n s.sh",
            "xargs --help",
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
