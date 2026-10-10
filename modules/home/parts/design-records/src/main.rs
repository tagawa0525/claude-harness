//! design-records: 設計の記録（ADR）の雛形を作り、検査する（ADR-0009）。
//!
//! Usage:
//!   design-records new <slug> <title>  docs/adr/NNNN-<slug>.md を作り、リポジトリの直下からのパスを出す
//!   design-records check [--changed] [FILE]...
//!                                      docs/adr の ADR を検査する。FILE（リポジトリの直下からのパス。pre-commit が
//!                                      渡すステージ済みのファイル）からは置き換えられた ADR への参照を探し、FILE の
//!                                      ADR は確定した後の本文の変更を見る。FILE か --changed を渡すと、warning は
//!                                      FILE のものだけ（--changed は FILE が 0 件のときも。pre-commit で使う）
//!   design-records status              標準入力の文書の前付けの status を出す（無ければ -）
//!
//! 終了コード: 0 = 成功（注意だけのときも）、1 = 違反か失敗、2 = 使い方の誤り

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Command, ExitCode};

use design_records::check::{Finding, Severity, body_changed, check};
use design_records::{
    ADR_DIR, OPEN_STATUSES, adr_path, is_adr_path, is_slug, is_title, next_number,
};

type Result<T> = std::result::Result<T, String>;

const USAGE: &str =
    "Usage: design-records new <slug> <title> | check [--changed] [FILE]... | status";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["new", slug, title] if !title.is_empty() => new(slug, title),
        ["check", "--changed", files @ ..] => run_check(files, true),
        ["check", files @ ..] if !files.iter().any(|f| f.starts_with('-')) => {
            run_check(files, !files.is_empty())
        }
        ["status"] => status(),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("design-records: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `git args…` を `dir` で走らせ、成功なら標準出力を返す。
fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        // 日本語のファイル名を引用符と 8 進数で包ませない（番号を読むため）
        .args(["-c", "core.quotePath=false"])
        .args(args)
        .output()
        .map_err(|e| format!("cannot run git (is git installed?): {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("git {}: {e}", args.join(" ")))
}

/// `git args…` が成功するか（失敗は答えの一つとして扱う）。
fn git_ok(dir: &Path, args: &[&str]) -> Result<bool> {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .map_err(|e| format!("cannot run git (is git installed?): {e}"))
}

/// 今いるリポジトリの直下。
fn repo_root() -> Result<std::path::PathBuf> {
    let top = git(Path::new("."), &["rev-parse", "--show-toplevel"])?;
    Ok(top.trim_end_matches('\n').into())
}

/// `dir` の直下のファイル名（ディレクトリが無ければ空）。
fn file_names(dir: &Path) -> Result<Vec<String>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        if entry.path().is_file() {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    Ok(names)
}

fn new(slug: &str, title: &str) -> Result<ExitCode> {
    if !is_slug(slug) {
        return Err(format!(
            "the slug must be lowercase kebab-case ([a-z0-9]+(-[a-z0-9]+)*): {slug}"
        ));
    }
    if !is_title(title) {
        return Err(format!("the title must be one non-blank line: {title:?}"));
    }
    let root = repo_root()?;
    // 削除した ADR の番号を使い回すと、古い参照が別の決定を指してしまうので、全ブランチの履歴も数える
    let history = git(
        &root,
        &["log", "--all", "--format=", "--name-only", "--", ADR_DIR],
    )?;
    let present: Vec<String> = file_names(&root.join(ADR_DIR))?
        .into_iter()
        .map(|name| format!("{ADR_DIR}/{name}"))
        .collect();
    let number = next_number(history.lines().chain(present.iter().map(String::as_str)));
    let date = today()?;
    let path = adr_path(number, slug);
    let file = root.join(&path);
    fs::create_dir_all(root.join(ADR_DIR)).map_err(|e| format!("{ADR_DIR}: {e}"))?;
    fs::write(&file, design_records::template(number, title, &date))
        .map_err(|e| format!("{path}: {e}"))?;
    println!("{path}");
    Ok(ExitCode::SUCCESS)
}

/// 手元の暦の今日（`date +%F`。標準ライブラリには時間帯の変換が無い）。
fn today() -> Result<String> {
    let output = Command::new("date")
        .arg("+%F")
        .output()
        .map_err(|e| format!("cannot run date: {e}"))?;
    if !output.status.success() {
        return Err("date +%F failed".to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// `changed_only` なら、warning は `files` のものだけを出す。
fn run_check(files: &[&str], changed_only: bool) -> Result<ExitCode> {
    let root = repo_root()?;
    let mut adrs = BTreeMap::new();
    for path in tracked(&root, ADR_DIR)? {
        let text = fs::read_to_string(root.join(&path)).map_err(|e| format!("{path}: {e}"))?;
        adrs.insert(path, text);
    }
    let issues = issue_numbers(&root)?;
    let mut scanned = BTreeMap::new();
    for path in files {
        // 中身の読めないもの（消したファイル、テキストでないファイル）は参照を書けないので見ない
        if let Ok(bytes) = fs::read(root.join(path))
            && let Ok(text) = String::from_utf8(bytes)
        {
            scanned.insert(path.to_string(), text);
        }
    }

    let mut found = check(&adrs, issues.as_ref(), &scanned);
    // FILE か --changed を渡したら（pre-commit）、warning はそのファイルのものだけにする。触っていない ADR の warning が毎回
    // 並ぶと、今のコミットの warning が埋もれる。error は ADR どうしの整合なので、すべて出す
    if changed_only {
        found.retain(|f| f.severity == Severity::Error || files.contains(&f.path.as_str()));
    }
    found.extend(body_changes(&root, &adrs, files)?);
    for finding in &found {
        println!("{finding}");
    }
    Ok(if found.iter().any(|f| f.severity == Severity::Error) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// `dir` の直下の、追跡している（ステージ済みを含む）.md のうち、作業ツリーにあるもの。prek は追跡していない
/// ファイルを退避しないので、作業ツリーをそのまま読むと、コミットに入らない下書きで結果が変わる。
fn tracked(root: &Path, dir: &str) -> Result<Vec<String>> {
    let listed = git(root, &["ls-files", "-z", "--", dir])?;
    Ok(listed
        .split('\0')
        .filter(|p| {
            p.strip_prefix(dir)
                .and_then(|rest| rest.strip_prefix('/'))
                .is_some_and(|name| !name.contains('/') && name.ends_with(".md"))
                && root.join(p).is_file()
        })
        .map(str::to_string)
        .collect())
}

/// 追跡している docs/issues があれば、その issue の番号（`NNNN-*.md`）。無ければ `None`（GitHub などの issue を
/// 使う）。
fn issue_numbers(root: &Path) -> Result<Option<BTreeSet<u32>>> {
    let paths = tracked(root, "docs/issues")?;
    if paths.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        paths
            .iter()
            .filter_map(|path| {
                let name = path.rsplit('/').next()?;
                name.strip_suffix(".md")?.split_once('-')?.0.parse().ok()
            })
            .collect(),
    ))
}

/// 既定ブランチ。.claude/scripts/branch-topics.sh、worktree-add.sh、claude-hooks（git.rs）と同じ順で探す。
fn default_branch(root: &Path) -> Result<Option<String>> {
    if let Ok(head) = git(
        root,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    ) {
        return Ok(Some(head.trim().to_string()));
    }
    for name in ["origin/main", "origin/master", "main", "master"] {
        let full = if name.contains('/') {
            format!("refs/remotes/{name}")
        } else {
            format!("refs/heads/{name}")
        };
        if git_ok(root, &["show-ref", "--verify", "--quiet", &full])? {
            return Ok(Some(name.to_string()));
        }
    }
    Ok(None)
}

/// 確定した ADR の本文の変更（注意）。比べるのは既定ブランチとの分岐点（main にマージする前のブランチの上の
/// 変更は status によらず自由）。分岐点で決める前の status だった ADR と、分岐点に無い ADR は見ない。
fn body_changes(
    root: &Path,
    adrs: &BTreeMap<String, String>,
    files: &[&str],
) -> Result<Vec<Finding>> {
    let changed: Vec<&str> = files
        .iter()
        .copied()
        .filter(|f| is_adr_path(f) && adrs.contains_key(*f))
        .collect();
    if changed.is_empty() || !git_ok(root, &["rev-parse", "--verify", "--quiet", "HEAD"])? {
        return Ok(Vec::new());
    }
    let (base, shown) = match default_branch(root)? {
        // 分岐点が無ければ（orphan のブランチ、浅い clone）、比べる版が無いので見ない（注意だけの検査で止めない）
        Some(branch) => match git(root, &["merge-base", "HEAD", &branch]) {
            Ok(base) => (base.trim().to_string(), branch),
            Err(_) => return Ok(Vec::new()),
        },
        None => ("HEAD".to_string(), "HEAD".to_string()),
    };
    let mut found = Vec::new();
    for path in changed {
        let spec = format!("{base}:{path}");
        if !git_ok(root, &["cat-file", "-e", &spec])? {
            continue;
        }
        let old = git(root, &["show", &spec])?;
        let Some(status) =
            design_records::status(&old).filter(|s| !OPEN_STATUSES.contains(&s.as_str()))
        else {
            continue;
        };
        if body_changed(&old, &adrs[path]) {
            found.push(Finding {
                path: path.to_string(),
                line: None,
                severity: Severity::Warning,
                message: format!(
                    "the body changed although the ADR is {status} on {shown}; \
                     after a decision, fix only typos, grammar, markup and broken links \
                     (append dated notes to the notes section), or supersede it with a new ADR"
                ),
            });
        }
    }
    Ok(found)
}

fn status() -> Result<ExitCode> {
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .map_err(|e| format!("cannot read stdin: {e}"))?;
    let status = design_records::status(&text);
    println!("{}", status.as_deref().unwrap_or("-"));
    Ok(ExitCode::SUCCESS)
}
