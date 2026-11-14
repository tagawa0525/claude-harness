//! `rename-plan.sh --list | <branch-name> [--file <path>]`: docs/plans/ のランダムな名前の計画書を、連番つきの名前に
//! する。Claude Code の plan mode は plansDirectory（docs/plans）にランダムな名前で計画書を置くので、ブランチ名が
//! 決まったら `NNN_<name>.md` にして追いやすくする。
//!
//! - --list: 連番（NNN_）の無い .md を新しい順に出す（ブランチ名を決める材料）
//! - 名前を変える対象は --file、省けばランダムな名前の計画書がちょうど 1 つのときそれ。複数なら 1
//! - NNN は既存の最大の番号 + 1（無ければ 001）
//! - name はブランチ名から先頭の type/ を除き、- と / を _ にしたもの
//! - git が追跡していれば git mv してコミットする。追跡していなければ名前だけ変える
//!
//! 出力: `RENAMED: <old> -> <new>`、`COMMITTED: yes|no`

use crate::git::git;
use std::path::Path;
use std::process::Command;

const USAGE: &str = "Usage: rename-plan.sh --list | <branch-name> [--file <path>]";
const PLANS_DIR: &str = "docs/plans";

fn usage() -> i32 {
    eprintln!("{USAGE}");
    1
}

fn numbered(name: &str) -> bool {
    name.len() >= 4
        && name.as_bytes()[..3].iter().all(u8::is_ascii_digit)
        && name.as_bytes()[3] == b'_'
}

/// 連番の無い計画書を新しい順に（リポジトリの直下からのパスで）
fn random_plans() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(PLANS_DIR) else {
        return Vec::new();
    };
    let mut plans: Vec<(std::time::SystemTime, String)> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            // symlink は先を辿る（計画書が別の場所への symlink でも数える）。隠しファイルは数えない（*.md の glob と同じ）
            let meta = std::fs::metadata(e.path()).ok()?;
            (meta.is_file() && !name.starts_with('.') && name.ends_with(".md") && !numbered(&name))
                .then(|| {
                    (
                        meta.modified().unwrap_or(std::time::UNIX_EPOCH),
                        format!("{PLANS_DIR}/{name}"),
                    )
                })
        })
        .collect();
    plans.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    plans.into_iter().map(|(_, p)| p).collect()
}

/// git を標準入出力を引き継いで起動し、失敗すればその終了コード
fn step(args: &[&str]) -> Result<(), i32> {
    match Command::new("git").args(args).status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(crate::child::exit_code(s)),
        Err(e) => {
            eprintln!("ERROR: git を起動できません: {e}");
            Err(1)
        }
    }
}

pub fn main(args: &[String]) -> i32 {
    let Some(root) = git(Path::new("."), &["rev-parse", "--show-toplevel"]) else {
        eprintln!("ERROR: git リポジトリではありません");
        return 1;
    };
    if let Err(e) = std::env::set_current_dir(&root) {
        eprintln!("ERROR: {root} に移れません: {e}");
        return 1;
    }
    let mut list = false;
    let mut branch: Option<&str> = None;
    let mut file: Option<&str> = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--list" => list = true,
            "--file" => match rest.next().filter(|f| !f.is_empty()) {
                Some(f) => file = Some(f),
                None => return usage(),
            },
            a if a.starts_with('-') => return usage(),
            a => {
                if branch.is_some() {
                    return usage();
                }
                branch = Some(a);
            }
        }
    }
    if list {
        for plan in random_plans() {
            println!("{plan}");
        }
        return 0;
    }
    let Some(branch) = branch.filter(|b| !b.is_empty()) else {
        return usage();
    };
    match rename(branch, file) {
        Ok(()) => 0,
        Err(code) => code,
    }
}

fn rename(branch: &str, file: Option<&str>) -> Result<(), i32> {
    if !Path::new(PLANS_DIR).is_dir() {
        println!("NOTE: {PLANS_DIR} がありません。リネーム対象なし");
        return Ok(());
    }
    let file = match file {
        Some(f) => f.to_string(),
        None => {
            let candidates = random_plans();
            match candidates.as_slice() {
                [] => {
                    println!("NOTE: リネーム対象の計画書はありません");
                    return Ok(());
                }
                [one] => one.clone(),
                many => {
                    eprintln!(
                        "ERROR: ランダム名の計画書が複数あります。--file で対象を指定してください:"
                    );
                    for c in many {
                        eprintln!("  {c}");
                    }
                    return Err(1);
                }
            }
        }
    };
    if !Path::new(&file).is_file() {
        eprintln!("ERROR: ファイルがありません: {file}");
        return Err(1);
    }
    let max = std::fs::read_dir(PLANS_DIR)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    (numbered(&name) && name.ends_with(".md"))
                        .then(|| name[..3].parse::<u32>().ok())
                        .flatten()
                })
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    let name = branch.split_once('/').map_or(branch, |(_, rest)| rest);
    let name = name.replace(['-', '/'], "_");
    let new = format!("{PLANS_DIR}/{:03}_{name}.md", max + 1);
    if Path::new(&new).exists() {
        eprintln!("ERROR: 既に存在します: {new}");
        return Err(1);
    }
    let here = Path::new(".");
    let tracked = git(here, &["ls-files", "--error-unmatch", "--", &file]).is_some();
    let committed = if tracked {
        step(&["mv", "--", &file, &new])?;
        let stem = |p: &str| {
            Path::new(p)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        let new_name = Path::new(&new)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        step(&[
            "commit",
            "-q",
            "--only",
            "-m",
            &format!("docs: rename plan {} to {new_name}", stem(&file)),
            "--",
            &file,
            &new,
        ])?;
        "yes"
    } else {
        if let Err(e) = std::fs::rename(&file, &new) {
            eprintln!("ERROR: {file} を {new} にできません: {e}");
            return Err(1);
        }
        "no"
    };
    println!("RENAMED: {file} -> {new}");
    println!("COMMITTED: {committed}");
    Ok(())
}
