//! `resume-state.sh`: 再開時に、開いている PR、worktree、Slurm のジョブ、docs/issues の issue を 1 つの出力に集める
//! （resume のスキルの `!` 行が呼ぶ）。対象のリポジトリの中で実行する。
//!
//! 出力は節ごとに `== <名前>` の見出しを置き、集められないものは `(なし)` か理由の 1 行にする。情報を集めるだけなので、
//! 個々のコマンドの失敗では止めない（リポジトリの外で docs/issues の場所が決まらないときだけ 1 で終える）。
//!
//! - `== worktrees`: `<パス> <ブランチ> [dirty] [session=<Claude-Session>]`（先端のコミットの trailer。無ければ省く）
//! - `== prs`: gh pr list の開いている PR（GitHub の URL を持つ remote と gh があるときだけ。remote の名前は問わない）
//! - `== slurm`: squeue の自分のジョブ
//! - `== issues`: docs/issues の status: open の issue（blocked-by があれば添える）。blocked-by の `#NNNN` の相手を
//!   docs/issues の同じ番号の記録で引き、閉じていない相手（duplicate で閉じた相手は開いている重複先）を `blocked=`、閉じたかを確かめられない相手（記録が無い、
//!   status が open でも closed でもない、duplicate の重複先を読めない、たどると自分に戻る、`#NNNN` の形でない、
//!   blocked-by を読めない）を `blocker-unknown=` で添え、読めないファイルは `unreadable=` で出し、印があれば節の最後に、
//!   その印の issue の扱いを `NEXT:` で出す。status、番号、参照の読み方は design-records に合わせる
//!
//! AWS の監視など、プロジェクト固有の状態は集めない（そのリポジトリの README か計画に従う）。

use design_records::Kind;
use design_records::parse::{FrontMatter, Value, numbered_refs, parse_front_matter};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    // 実効の利用者は id で引く（USER は su や sudo -E の後に元の利用者のまま残る）。id が無い環境だけ環境変数を見る
    let user = Command::new("id")
        .arg("-un")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|u| !u.is_empty())
        .or_else(|| {
            ["USER", "USERNAME"]
                .iter()
                .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        })
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

/// 先頭のフロントマター（1 行目の `---` から次の `---` まで）の `<key>:` の行の値（そのまま）。表示に使う
fn raw_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let mut lines = text.lines();
    if lines.next() != Some("---") {
        return None;
    }
    lines
        .take_while(|l| *l != "---")
        .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))
        .map(|v| v.trim_start_matches(' '))
}

/// `#NNNN` だけの参照の番号（design-records の参照と同じく 4 桁）
fn issue_ref(item: &str) -> Option<u32> {
    match numbered_refs(item, "#").as_slice() {
        [(0, n)] if item.len() == 5 => Some(*n),
        _ => None,
    }
}

/// 番号ごとの記録の、閉じたかの判定に要るもの
struct Record {
    /// status（読めなければ None）
    status: Option<String>,
    /// duplicate で閉じたときの重複先
    duplicate_of: Vec<u32>,
}

/// 相手の状態
enum Blocker {
    Closed,
    /// 開いている記録の番号（duplicate で閉じた相手は、たどった先の重複先）
    Open(u32),
    Unknown,
}

/// 相手が閉じているか。duplicate で閉じた相手は、仕事が重複先に移っているので、すべての重複先をたどって決める。
/// seen は、たどってきた番号と自分（自分にたどり着いたら、自分を待つことになるので確かめられないものとする）
fn blocker(records: &HashMap<u32, Vec<Record>>, n: u32, seen: &[u32]) -> Blocker {
    if seen.contains(&n) {
        return Blocker::Unknown;
    }
    let Some(rs) = records.get(&n) else {
        return Blocker::Unknown;
    };
    // 同じ番号の記録が複数あれば、どれかが開いていれば開いているとみなす
    if rs.iter().any(|r| r.status.as_deref() == Some("open")) {
        return Blocker::Open(n);
    }
    // open でも closed でもない status は、閉じたかを決められない
    if rs.iter().any(|r| r.status.as_deref() != Some("closed")) {
        return Blocker::Unknown;
    }
    let seen: Vec<u32> = seen.iter().copied().chain([n]).collect();
    let mut unknown = false;
    for next in rs.iter().flat_map(|r| &r.duplicate_of) {
        match blocker(records, *next, &seen) {
            Blocker::Open(m) => return Blocker::Open(m),
            Blocker::Unknown => unknown = true,
            Blocker::Closed => {}
        }
    }
    if unknown {
        Blocker::Unknown
    } else {
        Blocker::Closed
    }
}

/// docs/issues の 1 つのファイル
struct Entry {
    name: String,
    /// 記録の番号（`NNNN-slug.md` の形でなければ None）
    number: Option<u32>,
    /// 本文（読めなければ理由）
    text: Result<String, String>,
}

/// 閉じた記録の重複先。duplicate で閉じたのに重複先を読めなければ None
fn duplicate_targets(front: &FrontMatter) -> Option<Vec<u32>> {
    if front.get("resolution") != Some(&Value::Scalar("duplicate".into())) {
        return Some(Vec::new());
    }
    let items = front
        .get("duplicate-of")
        .map(Value::items)
        .unwrap_or_default();
    if items.is_empty() {
        return None;
    }
    items.into_iter().map(issue_ref).collect()
}

fn issues(root: &Path) {
    let dir = Kind::Issue.dir();
    let mut files: Vec<PathBuf> = std::fs::read_dir(root.join(dir))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "md") && p.is_file())
        .collect();
    files.sort();
    let entries: Vec<Entry> = files
        .iter()
        .map(|f| {
            let name = f
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let number =
                design_records::record_name(Kind::Issue, &format!("{dir}/{name}")).map(|(n, _)| n);
            let text = std::fs::read_to_string(f).map_err(|e| e.to_string());
            Entry { name, number, text }
        })
        .collect();
    let mut records: HashMap<u32, Vec<Record>> = HashMap::new();
    for e in &entries {
        let Some(n) = e.number else { continue };
        let record = match &e.text {
            Err(_) => Record {
                status: None,
                duplicate_of: Vec::new(),
            },
            Ok(text) => {
                let status = design_records::status(text);
                let closed = status.as_deref() == Some("closed");
                // 閉じた記録は、フロントマターか重複先を読めなければ、仕事が済んだかを確かめられないので、status を
                // 読めないものとして扱う
                match parse_front_matter(text)
                    .ok()
                    .as_ref()
                    .map(duplicate_targets)
                {
                    Some(Some(targets)) => Record {
                        status,
                        duplicate_of: targets,
                    },
                    _ if closed => Record {
                        status: None,
                        duplicate_of: Vec::new(),
                    },
                    _ => Record {
                        status,
                        duplicate_of: Vec::new(),
                    },
                }
            }
        };
        records.entry(n).or_default().push(record);
    }
    let (mut found, mut any_blocked, mut any_unknown) = (false, false, false);
    for e in &entries {
        let text = match &e.text {
            Ok(text) => text,
            // 開いているかを決められないので、落とさずに出す
            Err(why) => {
                println!("{dir}/{} unreadable={why}", e.name);
                found = true;
                continue;
            }
        };
        if design_records::status(text).as_deref() != Some("open") {
            continue;
        }
        let mut line = format!("{dir}/{}", e.name);
        let raw = raw_value(text, "blocked-by");
        if let Some(blocked) = raw.filter(|v| !v.is_empty() && *v != "[]") {
            line.push_str(&format!(" blocked-by={blocked}"));
        }
        let (mut open, mut unknown) = (Vec::new(), Vec::new());
        match parse_front_matter(text) {
            Ok(front) => {
                for item in front
                    .get("blocked-by")
                    .map(Value::items)
                    .unwrap_or_default()
                {
                    let seen: Vec<u32> = e.number.into_iter().collect();
                    match issue_ref(item).map_or(Blocker::Unknown, |n| blocker(&records, n, &seen))
                    {
                        Blocker::Closed => {}
                        Blocker::Open(n) => open.push(Kind::Issue.reference(n)),
                        Blocker::Unknown => unknown.push(item.to_string()),
                    }
                }
            }
            // 読めないフロントマターの blocked-by は、相手を決められない（空の値は、続く行にブロックの一覧を書いた
            // ことがあるので、[] だけを相手なしとする）
            Err(_) if raw.is_some_and(|v| v != "[]") => unknown.push("unparsed".into()),
            Err(_) => {}
        }
        if !open.is_empty() {
            line.push_str(&format!(" blocked={}", open.join(",")));
            any_blocked = true;
        }
        if !unknown.is_empty() {
            line.push_str(&format!(" blocker-unknown={}", unknown.join(",")));
            any_unknown = true;
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
    if any_unknown {
        println!(
            "NEXT: blocker-unknown= の issue は、相手が閉じたかを確かめられない（相手の記録が無い、status か duplicate-of を読めない、#NNNN の形でない、たどると自分に戻る、blocked-by を読めない）。着手の前に blocked-by か相手の記録を直すか、ユーザーに確かめる"
        );
    }
}
