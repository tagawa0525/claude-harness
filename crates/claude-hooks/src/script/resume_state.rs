//! `resume-state.sh`: 再開時に、開いている PR、worktree、Slurm のジョブ、docs/issues の issue を 1 つの出力に集める
//! （resume のスキルの `!` 行が呼ぶ）。対象のリポジトリの中で実行する。
//!
//! 出力は節ごとに `== <名前>` の見出しを置き、集められないものは `(なし)` か理由の 1 行にする。情報を集めるだけなので、
//! 個々のコマンドの失敗では止めない（リポジトリの外で docs/issues の場所が決まらないときだけ 1 で終える）。
//!
//! - `== worktrees`: `<パス> <ブランチ> [dirty] [session=<Claude-Session>]`（先端のコミットの trailer。無ければ省く）
//! - `== prs`: gh pr list の開いている PR（GitHub の URL を持つ remote と gh があるときだけ。remote の名前は問わない）
//! - `== slurm`: squeue の自分のジョブ
//! - `== issues`: docs/issues の status: open の issue（blocked-by があれば添える）。blocked-by の `#<番号>` の相手を
//!   docs/issues の同じ番号のファイルで引き、閉じていない相手を `blocked=`、見つからない相手を `blocker-missing=` で
//!   添え、印があれば節の最後に、その印の issue の扱いを `NEXT:` で出す
//!
//! AWS の監視など、プロジェクト固有の状態は集めない（そのリポジトリの README か計画に従う）。

use regex::Regex;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

static ISSUE_REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#(\d+)").unwrap());

pub fn main(_args: &[String]) -> i32 {
    let here = Path::new(".");
    println!("== worktrees");
    worktrees(here);
    println!("== prs");
    prs(here);
    println!("== slurm");
    slurm();
    println!("== issues");
    let Some(root) = crate::git::git(here, &["rev-parse", "--show-toplevel"]) else {
        return 1;
    };
    issues(Path::new(&root));
    0
}

fn worktrees(here: &Path) {
    let list = crate::git::git(here, &["worktree", "list", "--porcelain"]).unwrap_or_default();
    let mut path: Option<String> = None;
    for line in list.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(p.to_string());
            continue;
        }
        let branch = if let Some(b) = line.strip_prefix("branch ") {
            b.strip_prefix("refs/heads/").unwrap_or(b).to_string()
        } else if line == "detached" {
            "(detached)".to_string()
        } else {
            continue;
        };
        let Some(p) = path.take() else { continue };
        let dir = Path::new(&p);
        let mut out = format!("{p} {branch}");
        if crate::git::git(dir, &["status", "--porcelain"]).is_some_and(|s| !s.is_empty()) {
            out.push_str(" dirty");
        }
        let message = crate::git::git(dir, &["log", "-1", "--format=%B"]).unwrap_or_default();
        if let Some(session) = message
            .lines()
            .find_map(|l| l.strip_prefix("Claude-Session: "))
        {
            out.push_str(&format!(" session={session}"));
        }
        println!("{out}");
    }
}

fn prs(here: &Path) {
    let gh = crate::staged_checks::find_on_path("gh");
    let (Some(gh), true) = (gh, crate::git::has_github_remote(here)) else {
        println!("(なし: gh か GitHub の remote が無い)");
        return;
    };
    let out = Command::new(gh)
        .args([
            "pr",
            "list",
            "--state",
            "open",
            "--json",
            "number,title,headRefName",
            "--template",
            r#"{{range .}}#{{.number}} {{.headRefName}} {{.title}}{{"\n"}}{{end}}"#,
        ])
        .output();
    match out {
        Ok(o) => {
            // 失敗の理由も見えるよう、標準エラーも出す
            print!("{}", String::from_utf8_lossy(&o.stdout));
            print!("{}", String::from_utf8_lossy(&o.stderr));
            if !o.status.success() {
                println!("(gh pr list に失敗)");
            }
        }
        Err(_) => println!("(gh pr list に失敗)"),
    }
}

fn slurm() {
    let Some(squeue) = crate::staged_checks::find_on_path("squeue") else {
        println!("(なし: squeue が無い)");
        return;
    };
    let user = Command::new("id")
        .arg("-un")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    match Command::new(squeue).args(["-h", "-u", &user]).output() {
        Ok(o) => {
            print!("{}", String::from_utf8_lossy(&o.stdout));
            print!("{}", String::from_utf8_lossy(&o.stderr));
            if !o.status.success() {
                println!("(squeue に失敗)");
            }
        }
        Err(_) => println!("(squeue に失敗)"),
    }
}

/// 先頭のフロントマター（1 行目の `---` から次の `---` まで）の行。1 行目が `---` でなければ空
fn front_matter(text: &str) -> Vec<&str> {
    let mut lines = text.lines();
    if lines.next() != Some("---") {
        return Vec::new();
    }
    lines.take_while(|l| *l != "---").collect()
}

fn value<'a>(front: &[&'a str], key: &str) -> Option<&'a str> {
    front.iter().find_map(|l| {
        l.strip_prefix(key)
            .and_then(|r| r.strip_prefix(':'))
            .map(|v| v.trim_start_matches(' '))
    })
}

fn is_open(front: &[&str]) -> bool {
    value(front, "status").is_some_and(|v| v == "open" || v == "\"open\"")
}

/// ファイルの名前の先頭の番号（`0003-slug.md` の 3）
fn number_of(path: &Path) -> Option<u64> {
    let name = path.file_name()?.to_str()?;
    let digits: String = name.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn issues(root: &Path) {
    let mut files: Vec<PathBuf> = std::fs::read_dir(root.join("docs/issues"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "md") && p.is_file())
        .collect();
    files.sort();
    let texts: Vec<String> = files
        .iter()
        .map(|f| std::fs::read_to_string(f).unwrap_or_default())
        .collect();
    // 番号ごとに、閉じているか（同じ番号のファイルが複数あれば、どれかが閉じていなければ閉じていないとみなす）
    let mut closed: std::collections::HashMap<u64, bool> = std::collections::HashMap::new();
    for (f, text) in files.iter().zip(&texts) {
        if let Some(n) = number_of(f) {
            let is_closed = value(&front_matter(text), "status")
                .is_some_and(|v| v == "closed" || v == "\"closed\"");
            closed
                .entry(n)
                .and_modify(|c| *c &= is_closed)
                .or_insert(is_closed);
        }
    }
    let (mut found, mut any_blocked, mut any_missing) = (false, false, false);
    for (f, text) in files.iter().zip(&texts) {
        let front = front_matter(text);
        if !is_open(&front) {
            continue;
        }
        let name = f.file_name().unwrap_or_default().to_string_lossy();
        let mut line = format!("docs/issues/{name}");
        if let Some(blocked) = value(&front, "blocked-by").filter(|v| !v.is_empty() && *v != "[]") {
            line.push_str(&format!(" blocked-by={blocked}"));
            let (mut open, mut missing) = (Vec::new(), Vec::new());
            for c in ISSUE_REF.captures_iter(blocked) {
                let shown = c[0].to_string();
                match c[1].parse::<u64>().ok().and_then(|n| closed.get(&n)) {
                    Some(true) => {}
                    Some(false) => open.push(shown),
                    None => missing.push(shown),
                }
            }
            if !open.is_empty() {
                line.push_str(&format!(" blocked={}", open.join(",")));
                any_blocked = true;
            }
            if !missing.is_empty() {
                line.push_str(&format!(" blocker-missing={}", missing.join(",")));
                any_missing = true;
            }
        }
        println!("{line}");
        found = true;
    }
    if !found {
        println!("(なし)");
    }
    if any_blocked {
        println!("NEXT: blocked= の issue は、そこに挙げた issue が閉じるまで着手しない");
    }
    if any_missing {
        println!(
            "NEXT: blocker-missing= の issue は、blocked-by の相手が docs/issues に無い。閉じたかを確かめられないので、着手の前に blocked-by を直すかユーザーに確かめる"
        );
    }
}
