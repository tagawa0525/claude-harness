//! design-records: 設計の記録（ADR と docs/issues の issue）のテンプレートを作り、番号を予約し、検査する（ADR-0009、ADR-0018）。
//!
//! Usage（どのコマンドも、前に `-C <dir>` を付けると `<dir>` で実行する。git の `-C` と同じ）:
//!   design-records new [--kind adr|issue] <slug> <title>
//!                                      番号を予約して <dir>/NNNN-<slug>.md を作り、リポジトリの直下からのパスを出す
//!                                      （dir は docs/adr か docs/issues。省けば adr）
//!   design-records renumber [--kind adr|issue] <old> [<new>]
//!                                      このブランチで足した記録 <old> を <new>（省けば次の番号）に振り直し、
//!                                      ファイル名と参照を付け替えて 1 コミットにする。新しいパスを出す
//!   design-records prune               既定ブランチの履歴に記録が入った予約のタグを、ローカルと origin から消す
//!   design-records check [--changed] [FILE]...
//!                                      docs/adr の ADR と docs/issues の番号を検査する。FILE（リポジトリの直下からの
//!                                      パス。pre-commit が渡すステージ済みのファイル）からは置き換えられた ADR への
//!                                      参照を探し、FILE の ADR は確定した後の本文の変更を見る。FILE か --changed を
//!                                      渡すと、warning は FILE のものだけ（--changed は FILE が 0 件のときも。
//!                                      pre-commit で使う）
//!   design-records status              標準入力の文書のフロントマターの status を出す（無ければ -）
//!
//! 終了コード: 0 = 成功（注意だけのときも）、1 = 違反か失敗、2 = 使い方の誤り

mod git;
mod numbering;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Command, ExitCode};

use design_records::check::{
    Finding, Severity, body_changed, check, check_issue_numbers, check_reservations,
};
use design_records::{
    ADR_DIR, ISSUE_DIR, Kind, OPEN_STATUSES, is_adr_path, is_slug, is_title, numbers_in,
};

use git::{Result, default_branch, git, git_ok, has_origin, repo_root, reservations, tracked};

const USAGE: &str = "Usage: design-records [-C <dir>] new [--kind adr|issue] <slug> <title> \
     | renumber [--kind adr|issue] <old> [<new>] | prune | check [--changed] [FILE]... | status";

/// 使い方の誤り。
struct UsageError;

/// サブコマンドの後ろの `--kind <adr|issue>`（省けば adr）と、残りの引数。
fn kind_and_rest<'a>(
    args: &'a [&'a str],
) -> std::result::Result<(Kind, &'a [&'a str]), UsageError> {
    match args {
        ["--kind", name, rest @ ..] => Ok((Kind::parse(name).ok_or(UsageError)?, rest)),
        rest => Ok((Kind::Adr, rest)),
    }
}

/// 4 桁までの番号（`18`、`0018`）。
fn number(arg: &str) -> std::result::Result<u32, UsageError> {
    if !(1..=4).contains(&arg.len()) || !arg.bytes().all(|b| b.is_ascii_digit()) {
        return Err(UsageError);
    }
    arg.parse().ok().filter(|&n| n > 0).ok_or(UsageError)
}

fn dispatch(args: &[&str]) -> std::result::Result<Result<ExitCode>, UsageError> {
    Ok(match args {
        ["new", rest @ ..] => match kind_and_rest(rest)? {
            (kind, [slug, title]) if !title.is_empty() => new(kind, slug, title),
            _ => return Err(UsageError),
        },
        ["renumber", rest @ ..] => match kind_and_rest(rest)? {
            (kind, [old]) => numbering::renumber(kind, number(old)?, None),
            (kind, [old, new]) => numbering::renumber(kind, number(old)?, Some(number(new)?)),
            _ => return Err(UsageError),
        },
        ["prune"] => numbering::prune(),
        ["check", "--changed", files @ ..] => run_check(files, true),
        ["check", files @ ..] if !files.iter().any(|f| f.starts_with('-')) => {
            run_check(files, !files.is_empty())
        }
        ["status"] => status(),
        _ => return Err(UsageError),
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut args: Vec<&str> = args.iter().map(String::as_str).collect();
    if let ["-C", dir, ..] = args.as_slice() {
        if let Err(e) = std::env::set_current_dir(dir) {
            eprintln!("design-records: cannot change to {dir}: {e}");
            return ExitCode::FAILURE;
        }
        args.drain(..2);
    }
    let Ok(result) = dispatch(&args) else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("design-records: {e}");
            ExitCode::FAILURE
        }
    }
}

fn new(kind: Kind, slug: &str, title: &str) -> Result<ExitCode> {
    if !is_slug(slug) {
        return Err(format!(
            "the slug must be lowercase kebab-case ([a-z0-9]+(-[a-z0-9]+)*): {slug}"
        ));
    }
    if !is_title(title) {
        return Err(format!("the title must be one non-blank line: {title:?}"));
    }
    numbering::new(kind, slug, title, &today()?)
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
    let issue_paths = tracked(&root, ISSUE_DIR)?;
    let issues = issue_numbers(&issue_paths);
    let adr_paths: Vec<String> = adrs.keys().cloned().collect();
    let mut reserved: BTreeMap<Kind, BTreeMap<u32, String>> = BTreeMap::new();
    for reservation in reservations(&root)? {
        if let Some(slug) = reservation.slug {
            reserved
                .entry(reservation.kind)
                .or_default()
                .insert(reservation.number, slug);
        }
    }
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
    found.extend(check_issue_numbers(&issue_paths));
    let origin = has_origin(&root)?;
    for (kind, paths) in [(Kind::Adr, &adr_paths), (Kind::Issue, &issue_paths)] {
        if let Some(reserved) = reserved.get(&kind) {
            found.extend(check_reservations(kind, paths, reserved, origin));
        }
    }
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

/// 追跡している docs/issues の issue（`paths`）があれば、その番号（番号を振るときと同じ [`numbers_in`] の規則）。
/// 無ければ `None`（GitHub などの issue を使う）。
fn issue_numbers(paths: &[String]) -> Option<BTreeSet<u32>> {
    (!paths.is_empty()).then(|| numbers_in(Kind::Issue, paths.iter().map(String::as_str)).collect())
}

/// 確定した ADR の本文の変更と削除（注意）。比べるのは既定ブランチとの分岐点（main にマージする前のブランチの上の
/// 変更は status によらず自由）。分岐点で決める前の status だった ADR と、分岐点に無い ADR は見ない。本文は `files`
/// の ADR だけを比べ（改名は改名前の版と比べる）、削除は渡されたかによらず示す（pre-commit は消したファイルを
/// 渡さない）。
fn body_changes(
    root: &Path,
    adrs: &BTreeMap<String, String>,
    files: &[&str],
) -> Result<Vec<Finding>> {
    if !git_ok(root, &["rev-parse", "--verify", "--quiet", "HEAD"])? {
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
    // 分岐点から作業ツリーまでの変更（pre-commit の中では、ステージしていない変更は退避されている）。
    // 改名の検出は diff.renames の設定に左右されないよう明示する
    let diff = git(
        root,
        &[
            "diff",
            "-z",
            "--name-status",
            "--find-renames",
            &base,
            "--",
            ADR_DIR,
        ],
    )?;
    let mut fields = diff.split('\0').filter(|f| !f.is_empty());
    let mut found = Vec::new();
    while let Some(kind) = fields.next() {
        let old_path = fields.next().unwrap_or_default();
        let new_path = if kind.starts_with('R') || kind.starts_with('C') {
            fields.next().unwrap_or_default()
        } else {
            old_path
        };
        let deleted = kind == "D";
        if !is_adr_path(old_path) || !(deleted || files.contains(&new_path)) {
            continue;
        }
        let spec = format!("{base}:{old_path}");
        if !git_ok(root, &["cat-file", "-e", &spec])? {
            continue;
        }
        let old = git(root, &["show", &spec])?;
        let Some(status) =
            design_records::status(&old).filter(|s| !OPEN_STATUSES.contains(&s.as_str()))
        else {
            continue;
        };
        let message = if deleted {
            format!(
                "the ADR was deleted although it is {status} on {shown}; \
                 keep decided ADRs and supersede them with a new ADR"
            )
        } else if adrs
            .get(new_path)
            .is_some_and(|new| body_changed(&old, new))
        {
            format!(
                "the body changed although the ADR is {status} on {shown}; \
                 after a decision, fix only typos, grammar, markup and broken links \
                 (append dated notes to the notes section), or supersede it with a new ADR \
                 (to change only part of it, write a new ADR whose `amends` names it, \
                 and add the new ADR to this ADR's `amended-by` once it is accepted)"
            )
        } else {
            continue;
        };
        found.push(Finding {
            path: if deleted { old_path } else { new_path }.to_string(),
            line: None,
            severity: Severity::Warning,
            message,
        });
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
