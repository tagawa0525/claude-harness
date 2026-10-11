//! git のコマンドの引数を読む: グローバルオプション（`-C`、`-c`、`--config-env`）と、サブコマンドごとの値を
//! 取るオプション。短縮形のまとまりは値を取るオプションの所で読むのを止め（`-m"add a"` の a は値）、別の語で
//! 渡した値（`-m -n`、`-o --force`）と `--` より後はオプションとして読まない

use crate::shell::Arg;

/// git のグローバルオプションのうち、次の引数を値として取るもの（値を読む `-C`、`-c`、`--config-env` を除く）
const GLOBAL_TAKES_VALUE: &[&str] = &[
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--super-prefix",
    "--list-cmds",
];

/// `git [グローバルオプション] <サブコマンド> …` のグローバルオプションの読み取り
#[derive(Debug, Clone)]
pub struct GitInvocation {
    /// `-C <dir>` の値（順に、前のものからの相対）
    pub dirs: Vec<String>,
    /// `-c <設定>` / `-c<設定>` と `--config-env <設定>` / `--config-env=<設定>` の設定
    pub config: Vec<String>,
    /// サブコマンドの、引数の中の位置（無ければ None）
    pub sub: Option<usize>,
}

pub fn parse_invocation(args: &[Arg]) -> GitInvocation {
    let mut out = GitInvocation {
        dirs: Vec::new(),
        config: Vec::new(),
        sub: None,
    };
    let mut i = 0;
    while i < args.len() {
        let t = args[i].text.as_str();
        if !t.starts_with('-') {
            out.sub = Some(i);
            break;
        }
        let next = args.get(i + 1).map(|a| a.text.clone());
        match t {
            "-C" => {
                out.dirs.extend(next);
                i += 2;
            }
            "-c" | "--config-env" => {
                out.config.extend(next);
                i += 2;
            }
            _ if GLOBAL_TAKES_VALUE.contains(&t) => i += 2,
            _ => {
                if let Some(v) = t
                    .strip_prefix("--config-env=")
                    .or_else(|| t.strip_prefix("-c"))
                {
                    out.config.push(v.to_string());
                }
                i += 1;
            }
        }
    }
    out
}

/// サブコマンドの値を取るオプション（git の各サブコマンドの man から、規則が見るものだけ）
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

/// 引数を読んだ 1 つ
#[derive(Debug)]
pub enum Item<'a> {
    /// 短縮形のフラグ（まとまりの中の 1 文字）
    Short(char),
    /// 長い形（`--x=v` は `--x`）
    Long(&'a str),
    /// オプションでも、その値でもない引数
    Positional(&'a Arg),
}

/// サブコマンド `sub` の引数を、短縮形、長い形、位置引数に分けて読む。オプションの値は読み飛ばす
pub fn items<'a>(sub: &str, args: &'a [Arg]) -> Vec<Item<'a>> {
    let options = value_options(sub);
    let mut out = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let t = a.text.as_str();
        if t == "--" {
            out.extend(it.map(Item::Positional));
            break;
        }
        if let Some(long) = t.strip_prefix("--") {
            let name = &t[..2 + long.find('=').unwrap_or(long.len())];
            out.push(Item::Long(name));
            if name == t && options.longs.contains(&name) {
                it.next();
            }
            continue;
        }
        if !t.starts_with('-') || t == "-" {
            out.push(Item::Positional(a));
            continue;
        }
        let cluster = &t[1..];
        for (i, ch) in cluster.char_indices() {
            out.push(Item::Short(ch));
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
    out
}

/// サブコマンド `sub` の引数に、短縮形のフラグ `short` か、長い形のどれか（`--x` か `--x=…`）があるか
pub fn git_flag(sub: &str, args: &[Arg], short: Option<char>, longs: &[&str]) -> bool {
    items(sub, args).iter().any(|item| match item {
        Item::Short(ch) => Some(*ch) == short,
        Item::Long(name) => longs.contains(name),
        Item::Positional(_) => false,
    })
}

/// サブコマンド `sub` の位置引数（オプションとその値を除く）
pub fn positionals<'a>(sub: &str, args: &'a [Arg]) -> Vec<&'a Arg> {
    items(sub, args)
        .into_iter()
        .filter_map(|item| match item {
            Item::Positional(a) => Some(a),
            _ => None,
        })
        .collect()
}
