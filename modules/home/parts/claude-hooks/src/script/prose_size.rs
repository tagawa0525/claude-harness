//! `claude-hooks script prose-size [--since <日時>] [--until <日時>] [--memory <ディレクトリ>] [--repo <パス>]...`: 区間の
//! 前後の文章（メモリ、スキル、CLAUDE.md）の大きさをバイトで出す（`.claude/skills/retro/scripts/prose-size.sh` が呼ぶ。
//! 使い方、数える範囲、出力の形は sh の冒頭）。
//!
//! 誤りは、sh の `set -e` と同じく、git の失敗はその終了コードで止め（それまでに出した行は残る）、リポジトリごとの
//! 誤り（既定ブランチが無い、共通の祖先が無い）は次のリポジトリに進んで最後に 1 で終える。git の標準エラーは
//! そのまま流す。

use super::time::{self, Time};
use std::path::Path;
use std::process::{Command, Stdio};

const USAGE: &str = "Usage: prose-size.sh [--since <日時>] [--until <日時>] [--memory <メモリのディレクトリ>] [--repo <パス>]...";

const BACKUP_REF: &str = "refs/memory-backup";

/// 止めるときの終了コード
type Exit = i32;

pub fn main(args: &[String]) -> i32 {
    match run(args) {
        Ok(code) | Err(code) => code,
    }
}

fn usage() -> Result<i32, Exit> {
    eprintln!("{USAGE}");
    Err(1)
}

fn error(message: String) -> Exit {
    eprintln!("ERROR: {message}");
    1
}

/// 時差つきの ISO 8601 を UNIX 時刻（秒）にする。秒の端数は切り捨てる（sh の `date +%s` と同じ）
fn to_epoch(name: &str, value: &str) -> Result<i64, Exit> {
    if !time::ISO_WITH_OFFSET.is_match(value) {
        return Err(error(format!(
            "{name} は時差つきの ISO 8601 で書いてください（例 2026-11-06T00:00+09:00、2026-11-05T15:00Z）: {value}"
        )));
    }
    match time::parse_time(value) {
        Some(t) => Ok(t.micros.div_euclid(1_000_000)),
        None => Err(error(format!("{name} の日時を読めません: {value}"))),
    }
}

fn jst(seconds: i64) -> String {
    time::jst_iso(Time {
        micros: seconds * 1_000_000,
    })
}

/// git の --before は指定の時刻ちょうどのコミットを含むので、1 秒前を渡して区間 [since, until) の外に置く
fn before_arg(seconds: i64) -> String {
    format!("--before=@{} +0000", seconds - 1)
}

fn spawn_failed(e: &std::io::Error) -> Exit {
    eprintln!("prose-size: git を起動できません: {e}");
    127
}

/// git の標準出力。失敗すれば、その終了コードで止める（標準エラーはそのまま流す）
fn git(dir: &str, args: &[&str]) -> Result<Vec<u8>, Exit> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| spawn_failed(&e))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(out.status.code().unwrap_or(1))
    }
}

/// 1 行の出力（末尾の改行を除く）
fn git_line(dir: &str, args: &[&str]) -> Result<String, Exit> {
    Ok(String::from_utf8_lossy(&git(dir, args)?)
        .trim_end()
        .to_string())
}

/// `rev-parse -q --verify <rev>^{commit}` が通るか（標準エラーは `quiet` なら捨てる）
fn has_commit(dir: &str, rev: &str, quiet: bool) -> Result<bool, Exit> {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "-q", "--verify", &format!("{rev}^{{commit}}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(if quiet {
            Stdio::null()
        } else {
            Stdio::inherit()
        })
        .status()
        .map_err(|e| spawn_failed(&e))?;
    Ok(status.success())
}

/// コミットの中で `cond` に合うパスのファイル（blob）の大きさの和。コミットが無ければ 0
fn tree_bytes(dir: &str, commit: &str, cond: &dyn Fn(&[u8]) -> bool) -> Result<i64, Exit> {
    if commit.is_empty() {
        return Ok(0);
    }
    let out = git(dir, &["ls-tree", "-r", "-l", "-z", commit])?;
    let mut sum = 0;
    // 1 件は `<mode> <type> <object> <size>\t<path>`
    for record in out.split(|&b| b == 0).filter(|r| !r.is_empty()) {
        let Some(tab) = record.iter().position(|&b| b == b'\t') else {
            continue;
        };
        let meta = String::from_utf8_lossy(&record[..tab]);
        let fields: Vec<&str> = meta.split_whitespace().collect();
        if fields.get(1) == Some(&"blob") && cond(&record[tab + 1..]) {
            sum += fields
                .get(3)
                .and_then(|n| n.parse::<i64>().ok())
                .unwrap_or(0);
        }
    }
    Ok(sum)
}

fn is_skill(path: &[u8]) -> bool {
    path.starts_with(b".claude/skills/") && path.ends_with(b".md")
}

fn is_claude_md(path: &[u8]) -> bool {
    path == b"CLAUDE.md" || path.ends_with(b"/CLAUDE.md")
}

fn delta(before: i64, after: i64) -> String {
    let d = after - before;
    if d > 0 {
        format!("+{d}")
    } else {
        d.to_string()
    }
}

/// 後の大きさを数えるコミット。`--until` があれば既定ブランチの until より前の最後のコミット、無ければ既定ブランチの
/// 今に、HEAD が分岐点から変えた分を足す（base + HEAD - 分岐点）
enum After {
    Until(String),
    Now {
        tip: String,
        head: String,
        fork: String,
    },
}

impl After {
    fn bytes(&self, repo: &str, cond: &dyn Fn(&[u8]) -> bool) -> Result<i64, Exit> {
        match self {
            After::Until(new) => tree_bytes(repo, new, cond),
            After::Now { tip, head, fork } => Ok(tree_bytes(repo, tip, cond)?
                + tree_bytes(repo, head, cond)?
                - tree_bytes(repo, fork, cond)?),
        }
    }
}

/// リポジトリの 2 行を出す。続けられる誤り（既定ブランチが無い、共通の祖先が無い）は Ok(false)
fn report_repo(repo: &str, since: i64, until: Option<i64>) -> Result<bool, Exit> {
    let mut base = None;
    for r in ["origin/HEAD", "main", "master"] {
        if has_commit(repo, r, false)? {
            base = Some(r);
            break;
        }
    }
    let Some(base) = base else {
        error(format!(
            "{repo} に既定ブランチ（origin/HEAD、main、master）がありません"
        ));
        return Ok(false);
    };
    let rev_list_before = |t: i64| {
        git_line(
            repo,
            &["rev-list", "-1", "--first-parent", &before_arg(t), base],
        )
    };
    let old = rev_list_before(since)?;
    let after = match until {
        Some(u) => After::Until(rev_list_before(u)?),
        None => {
            let tip = git_line(repo, &["rev-parse", base])?;
            let head = git_line(repo, &["rev-parse", "HEAD"])?;
            let Ok(fork) = git_line(repo, &["merge-base", &tip, &head]) else {
                error(format!(
                    "{repo} の HEAD と既定ブランチ（{base}）に共通の祖先が無いので、HEAD の変えた分が分かりません"
                ));
                return Ok(false);
            };
            After::Now { tip, head, fork }
        }
    };
    for (kind, cond) in [
        ("skills", &is_skill as &dyn Fn(&[u8]) -> bool),
        ("claude-md", &is_claude_md),
    ] {
        let a = tree_bytes(repo, &old, cond)?;
        let b = after.bytes(repo, cond)?;
        println!("PROSE: {kind} {a} {b} {} {repo}", delta(a, b));
    }
    Ok(true)
}

/// ディレクトリの下のすべての通常のファイル（symlink は辿らない。sh の `find -type f`）の大きさの和
fn disk_bytes(path: &Path) -> Result<i64, Exit> {
    let meta = std::fs::symlink_metadata(path).map_err(|e| read_failed(path, &e))?;
    if meta.is_file() {
        return Ok(meta.len() as i64);
    }
    if !meta.is_dir() {
        return Ok(0);
    }
    let mut sum = 0;
    for entry in std::fs::read_dir(path).map_err(|e| read_failed(path, &e))? {
        let entry = entry.map_err(|e| read_failed(path, &e))?;
        sum += disk_bytes(&entry.path())?;
    }
    Ok(sum)
}

fn read_failed(path: &Path, e: &std::io::Error) -> Exit {
    error(format!("{} を読めません: {e}", path.display()))
}

fn report_memory(memory: &str, since: i64, until: Option<i64>) -> Result<(), Exit> {
    let home = format!("{}/.claude", crate::shell::home().display());
    let Some(rel) = memory.strip_prefix(&format!("{home}/")) else {
        return Err(error(format!(
            "メモリのディレクトリが {home} の下にありません（退避は {home} の git にあります）: {memory}"
        )));
    };
    let has_ref = has_commit(&home, BACKUP_REF, true)?;
    let prefix = format!("{rel}/");
    // 退避の中身にこのディレクトリがあるか（退避を始める前の区間を、メモリの無いプロジェクトと見分ける）。
    // 読めなければ無いと数える（sh の `[[ -n "$(…)" ]]` は失敗で止まらない）
    let in_backup = has_ref
        && Command::new("git")
            .args([
                "-C",
                &home,
                "ls-tree",
                "-r",
                "--name-only",
                BACKUP_REF,
                "--",
            ])
            .arg(&prefix)
            .stdin(Stdio::null())
            .stderr(Stdio::inherit())
            .output()
            .is_ok_and(|o| !o.stdout.is_empty());
    let last_backup_before = |t: i64| -> Result<String, Exit> {
        if has_ref {
            git_line(&home, &["rev-list", "-1", &before_arg(t), BACKUP_REF])
        } else {
            Ok(String::new())
        }
    };
    let old = last_backup_before(since)?;
    let dir = Path::new(memory);
    if old.is_empty() && (dir.is_dir() || in_backup) {
        return Err(error(format!(
            "{} より前のメモリの退避（{home} の {BACKUP_REF}）が無いので、前の大きさが分かりません: {memory}",
            jst(since)
        )));
    }
    let under = |path: &[u8]| path.starts_with(prefix.as_bytes());
    let a = tree_bytes(&home, &old, &under)?;
    let b = match until {
        Some(u) => tree_bytes(&home, &last_backup_before(u)?, &under)?,
        None if dir.is_dir() => disk_bytes(dir)?,
        None => 0,
    };
    println!("PROSE: memory {a} {b} {} {memory}", delta(a, b));
    Ok(())
}

fn run(args: &[String]) -> Result<i32, Exit> {
    let mut since = None;
    let mut until = None;
    let mut memory = String::new();
    let mut repos = Vec::new();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let name = arg.as_str();
        if !matches!(name, "--since" | "--until" | "--memory" | "--repo") {
            return usage();
        }
        let Some(value) = it.next() else {
            return usage();
        };
        match name {
            "--since" => since = Some(to_epoch(name, value)?),
            "--until" => until = Some(to_epoch(name, value)?),
            "--memory" => memory = value.strip_suffix('/').unwrap_or(value).to_string(),
            _ => repos.push(value.clone()),
        }
    }
    if memory.is_empty() && repos.is_empty() {
        return usage();
    }
    let since = since.unwrap_or_else(|| {
        // JST の今日の 0 時
        let local = time::now_micros().div_euclid(1_000_000) + time::JST;
        local - local.rem_euclid(86_400) - time::JST
    });
    if let Some(u) = until
        && since >= u
    {
        return Err(error(format!(
            "--since（{}）は --until（{}）より前にしてください",
            jst(since),
            jst(u)
        )));
    }

    println!("SINCE: {}", jst(since));
    match until {
        Some(u) => println!("UNTIL: {}", jst(u)),
        None => println!("UNTIL: now"),
    }

    let mut failed = 0;
    for repo in &repos {
        if !report_repo(repo, since, until)? {
            failed = 1;
        }
    }
    if !memory.is_empty() {
        report_memory(&memory, since, until)?;
    }
    Ok(failed)
}
