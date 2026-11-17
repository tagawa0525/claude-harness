//! design-records: 設計の記録（ADR と docs/issues の issue）のテンプレートを作り、番号を予約し、検査する（ADR-0009、ADR-0018）。
//!
//! Usage（どのコマンドも、前に `-C <dir>` を付けると `<dir>` で実行する。git の `-C` と同じ）:
//!   design-records new [--kind adr|issue] <slug> <title>
//!                                      番号を予約して <dir>/NNNN-<slug>.md を作り、リポジトリの直下からのパスを出す
//!                                      （dir は docs/adr か docs/issues。省けば adr）。ADR の slug は決めたことを
//!                                      動詞で始めた英語の kebab-case。OKF のバンドルでは index と同じく索引の行も
//!                                      足し、一緒にコミットする索引を標準エラーの NEXT: に出す
//!   design-records renumber [--kind adr|issue] <old> [<new>]
//!                                      このブランチで足した記録 <old> を <new>（省けば次の番号）に振り直し、
//!                                      ファイル名と参照を付け替えて 1 コミットにする。新しいパスを出す
//!   design-records prune               既定ブランチの履歴に記録が入った予約のタグを、ローカルと origin から消す
//!   design-records prune --stale [--days N] [--delete]
//!                                      N 日（既定 30）より前の予約のうち、どのブランチの履歴にも記録が無いものを
//!                                      一覧にする。--delete で、ローカルと origin から消す。origin のブランチを
//!                                      取ってきて調べる
//!   design-records check [--changed] [FILE]...
//!                                      docs/adr の ADR と docs/issues の番号を検査する。FILE（リポジトリの直下からの
//!                                      パス。pre-commit が渡すステージ済みのファイル）からは置き換えられた ADR への
//!                                      参照を探し、FILE の ADR は確定した後の、決めた時点の記録（表題、背景、決定
//!                                      の要因、検討した案、決定と理由、各案の長所と短所、直せないフロントマターの
//!                                      キー）の変更と、決定と理由の変更に「## 変更」の項が足されたか（項の形は見ない）を見る。分岐点から足した amends は、FILE によらず全 ADR で見る（error）。FILE か
//!                                      --changed を渡すと、warning は FILE のものだけ（--changed は FILE が 0 件の
//!                                      ときも。pre-commit で使う）
//!   design-records index               OKF のバンドル（docs/index.md が okf_version を持つ docs/）の索引に、載っていない文書と下位の
//!                                      ディレクトリの行を足し、書いたパスを出す（ADR-0092）。`check` が同じバンドルの type と索引を検査する
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
    Finding, Severity, added_amends, body_changed, check, check_issue_format, check_issue_links,
    check_issue_numbers, check_reservations, confirmation_paths,
    decision_changed_without_change_item, superseded_dependents,
};
use design_records::okf::{BUNDLE_ROOT, check_bundle, complete_indexes, is_bundle_index};
use design_records::{
    ADR_DIR, AMENDABLE_KEYS, ISSUE_DIR, Kind, OPEN_STATUSES, RENUMBER_SUBJECT, is_adr_path,
    is_slug, is_title, numbers_in, record_name,
};

use git::{
    Result, default_branch, git, git_ok, has_origin, markdown_under, repo_root, reservations,
    tracked,
};

const USAGE: &str = "Usage: design-records [-C <dir>] new [--kind adr|issue] <slug> <title> \
     | renumber [--kind adr|issue] <old> [<new>] | prune [--stale [--days N] [--delete]] | check [--changed] [FILE]... | index | status";

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

/// 予約を使わずに置いておく日数の既定。担当が記録を作るまでに数日かかっても解放しない。
const STALE_DAYS: u64 = 30;

fn days(arg: &str) -> std::result::Result<u64, UsageError> {
    arg.parse().map_err(|_| UsageError)
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
        ["prune", "--stale", rest @ ..] => {
            let (days, delete) = match rest {
                [] => (STALE_DAYS, false),
                ["--delete"] => (STALE_DAYS, true),
                ["--days", n] => (days(n)?, false),
                ["--days", n, "--delete"] => (days(n)?, true),
                _ => return Err(UsageError),
            };
            numbering::prune_stale(days, delete)
        }
        ["check", "--changed", files @ ..] => run_check(files, true),
        ["check", files @ ..] if !files.iter().any(|f| f.starts_with('-')) => {
            run_check(files, !files.is_empty())
        }
        ["index"] => index(),
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
        // ADR の slug は決めたことを動詞で始める（`use-foo`）。kebab-case のほかは機械で見ないので、拒んだときに添える
        let verb = match kind {
            Kind::Adr => "; start it with the verb of the decision in English (use-foo, keep-bar)",
            Kind::Issue => "",
        };
        return Err(format!(
            "the slug must be lowercase kebab-case ([a-z0-9]+(-[a-z0-9]+)*){verb}: {slug}"
        ));
    }
    if !is_title(title) {
        return Err(format!("the title must be one non-blank line: {title:?}"));
    }
    let path = numbering::new(kind, slug, title, &today()?)?;
    println!("{path}");
    // OKF のバンドルでは、記録を足すと索引に載っていない文書になり、check が止める。index と同じ行を足し、
    // 一緒にコミットする索引を示す。パスを受ける呼び出し（`$(design-records new …)`）を壊さないよう、標準エラーに出す
    // 番号の予約と記録はもう済んでいるので、索引を書けなくても失敗で終えない（呼び直すと番号をもう 1 つ予約する）
    match repo_root().and_then(|root| write_indexes(&root, Some(&path))) {
        Ok(written) => {
            let written = written.unwrap_or_default();
            if !written.is_empty() {
                eprintln!(
                    "NEXT: commit the index lines added to {} together with {path}",
                    written.join(" ")
                );
            }
        }
        Err(e) => {
            eprintln!("design-records: cannot add the index lines for {path}: {e}");
            eprintln!(
                "NEXT: fix it, run `design-records index`, and commit the index lines it adds together with {path}"
            );
        }
    }
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
    found.extend(check_bundle(&bundle_files(&root, false)?));
    found.extend(check_issue_numbers(&issue_paths));
    let mut issue_texts = BTreeMap::new();
    for path in issue_paths
        .iter()
        .filter(|p| record_name(Kind::Issue, p).is_some())
    {
        // 追跡していても消した作業ツリーのファイルは読めない。形は見られないので飛ばす
        if let Ok(text) = fs::read_to_string(root.join(path)) {
            issue_texts.insert(path.clone(), text);
        }
    }
    found.extend(check_issue_format(&issue_texts));
    found.extend(check_issue_links(&issue_texts, &adr_paths));
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
    found.extend(branch_changes(&root, &adrs, files, changed_only)?);
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

/// 既定ブランチとの分岐点からの ADR の変更のうち、示すもの。
///
/// - 一部の置き換えの結び（`amends`）に足した ADR（違反。[`added_amends`]）。一部の変更は、変える ADR の「## 変更」に
///   書き、結びは分岐点に既にあるものだけを残す（ADR-0067）。status によらず、分岐点に無い ADR も見る
/// - 確定した ADR の決めた時点の記録（[`body_changed`]）の変更と削除（注意）と、決定と理由の変更に「## 変更」の項が
///   足されていないこと（注意。[`decision_changed_without_change_item`]）。main にマージする前のブランチの上の
///   変更は status によらず自由なので、分岐点で決める前の status だった ADR と、分岐点に無い ADR は見ない
/// - 分岐点から superseded になった ADR を、まだ `requires` に持つ ADR（注意。[`superseded_dependents`]）
///
/// 確認の節のファイルの注意（[`stale_confirmations`]）は、`changed_only` でなければ全 ADR で、そうなら `files` の ADR だけ見る。
/// `amends` の追加は `files` によらずすべての ADR で見る。凍結の注意は `files` の ADR だけを比べ（改名は改名前の版と
/// 比べる）、削除は渡されたかによらず示す（pre-commit は消した
/// ファイルを渡さない）。
fn branch_changes(
    root: &Path,
    adrs: &BTreeMap<String, String>,
    files: &[&str],
    changed_only: bool,
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
        if !is_adr_path(old_path) {
            continue;
        }
        // amends の追加は error なので、FILE によらず分岐点から変わったすべての ADR を見る（ほかの error と同じ）。
        // 凍結の注意（warning）は FILE の ADR だけにする。触っていない ADR の注意が並ぶと、今のコミットの注意が埋もれる
        let in_files = deleted || files.contains(&new_path);
        let spec = format!("{base}:{old_path}");
        let old = if git_ok(root, &["cat-file", "-e", &spec])? {
            Some(git(root, &["show", &spec])?)
        } else {
            None
        };
        if let Some(new) = adrs.get(new_path).filter(|_| !deleted) {
            for n in added_amends(old.as_deref(), new) {
                found.push(Finding {
                    path: new_path.to_string(),
                    line: None,
                    severity: Severity::Error,
                    message: format!(
                        "`amends` gains ADR-{n:04} since {shown}, but partial changes are no longer \
                         linked with amends and amended-by; drop the item and record the change in \
                         the ADR whose decision changes: edit its decision outcome and add a dated \
                         item to its `## 変更` section that quotes the replaced text and gives the \
                         reason"
                    ),
                });
            }
        }
        // FILE も --changed も無い check は、ほかの warning と同じく全 ADR を見る（凍結の注意は FILE の ADR だけ）
        if let Some(new) = adrs
            .get(new_path)
            .filter(|_| !deleted && (in_files || !changed_only))
        {
            found.extend(stale_confirmations(root, &base, new_path, new)?);
        }
        // ブランチで置き換えた ADR を、まだ前提にしている ADR。main で既に置き換えられていたものは一度見直したので
        // 出さない（requires は確定した後に直せず、毎回並ぶ）
        if let (Some(before), Some(now)) = (
            old.as_deref(),
            adrs.get(new_path)
                .filter(|_| !deleted && (in_files || !changed_only)),
        ) && design_records::status(before).as_deref() != Some("superseded")
            && design_records::status(now).as_deref() == Some("superseded")
        {
            let (dependents, successors) = superseded_dependents(adrs, new_path);
            if !dependents.is_empty() {
                let names = |ns: &[u32]| {
                    ns.iter()
                        .map(|n| format!("ADR-{n:04}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                found.push(Finding {
                    path: new_path.to_string(),
                    line: None,
                    severity: Severity::Warning,
                    message: format!(
                        "the ADR became superseded since {shown}; review whether the decisions of \
                         the ADRs that still list it in `requires` hold with {}, which supersedes \
                         it: {}",
                        names(&successors),
                        names(&dependents)
                    ),
                });
            }
        }
        let Some(old) = old.filter(|_| in_files) else {
            continue;
        };
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
                "the decided sections changed although the ADR is {status} on {shown}; \
                 the title, the front matter other than {}, and the context, decision drivers, \
                 considered options, decision outcome and pros and cons of the options \
                 record the decision as made, so change them only to fix typos, grammar, markup and \
                 broken links or to replace words with meaning-preserving ones from the Japanese \
                 dictionary (consequences, confirmation, notes and the `## 変更` section may follow \
                 the current facts). \
                 To change part of the decision, edit the decision outcome and add a dated \
                 item to the `## 変更` section that quotes the replaced text and gives the reason; \
                 to change all of it, supersede the ADR with a new one",
                AMENDABLE_KEYS.join(", ")
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
        if let Some(new) = adrs.get(new_path).filter(|_| !deleted)
            && decision_changed_without_change_item(&old, new)
        {
            found.push(Finding {
                path: new_path.to_string(),
                line: None,
                severity: Severity::Warning,
                message: "the decision outcome changed but no item was added to the `## 変更` \
                          section; add `- YYYY-MM-DD: <changed part>。前は「<quote of the previous \
                          text>」。<reason>`, or ignore this if the edit only fixes typos, grammar, \
                          markup or links"
                    .to_string(),
            });
        }
    }
    Ok(found)
}

/// ブランチが ADR の最後に変えたコミットより後に、ADR の「確認」の節に挙げたファイルを変えたコミットがあれば、その
/// ファイルごとの注意（#53）。実装を直したのに ADR の決定、帰結、確認が前の版のままになる食い違いを、書いた人の記憶に
/// 頼らず見つける。ADR を作業ツリーかインデックスで HEAD から変えているときは、ADR のほうが新しいので見ない。挙げたものが
/// HEAD のファイル（blob）でなければ見ない（コマンドや外のパスの可能性があり、実在でしか区別できない）。実装の変更が ADR の内容を
/// 変えるとは限らないので、直しを求めずに見直しを求める文にする。
fn stale_confirmations(root: &Path, base: &str, adr: &str, text: &str) -> Result<Vec<Finding>> {
    let range = format!("{base}..HEAD");
    // renumber のコミットは改名と参照の付け替えだけなので起点にしない。改名の前の版のコミットも辿る。--invert-grep で
    // 除くと --follow がその改名を見ずに辿りやめるので、件名を見て選ぶ
    let log = git(
        root,
        &["log", "--follow", "--format=%H %s", &range, "--", adr],
    )?;
    let last = log
        .lines()
        .filter_map(|l| l.split_once(' '))
        .find(|(_, subject)| !subject.starts_with(RENUMBER_SUBJECT))
        .map_or("", |(hash, _)| hash);
    // 作業ツリーかインデックスの ADR が HEAD と違えば、ADR のほうが新しい（直した版をこれからコミットする）ので見ない
    if last.is_empty() || !git_ok(root, &["diff", "--quiet", "HEAD", "--", adr])? {
        return Ok(Vec::new());
    }
    // 既定ブランチをマージして入った変更は、このブランチが実装を直したのではないので除く
    let (since, merged) = (format!("^{last}"), format!("^{base}"));
    let mut found = Vec::new();
    for path in confirmation_paths(text) {
        // ディレクトリ（tree）は含めない。`cat-file -e` は tree にも成功するので、種類を見る
        let is_file = git(root, &["cat-file", "-t", &format!("HEAD:{path}")])
            .is_ok_and(|kind| kind.trim() == "blob");
        if is_adr_path(&path) || !is_file {
            continue;
        }
        if !git(
            root,
            &[
                "log",
                "-1",
                "--format=%H",
                "HEAD",
                &since,
                &merged,
                "--",
                &path,
            ],
        )?
        .trim()
        .is_empty()
        {
            found.push(Finding {
                path: adr.to_string(),
                line: None,
                severity: Severity::Warning,
                message: format!(
                    "`{path}`, named in the confirmation section, changed in a later commit than \
                     the ADR; check that the decision, consequences and confirmation still match it"
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

/// docs/ の .md のパスと中身（`untracked` なら追跡していないものも含める）。
fn bundle_files(root: &Path, untracked: bool) -> Result<BTreeMap<String, String>> {
    let mut files = BTreeMap::new();
    // バンドルでなければ（docs/index.md が okf_version を持たなければ）、ほかの docs/ のファイルを読まない
    let root_index = match fs::read_to_string(root.join(BUNDLE_ROOT).join("index.md")) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(files),
        Err(e) => return Err(format!("{BUNDLE_ROOT}/index.md: {e}")),
    };
    if !is_bundle_index(&root_index) {
        return Ok(files);
    }
    for path in markdown_under(root, BUNDLE_ROOT, untracked)? {
        let text = fs::read_to_string(root.join(&path)).map_err(|e| format!("{path}: {e}"))?;
        files.insert(path, text);
    }
    Ok(files)
}

/// バンドルの索引に載っていない行を足し、書いたパスを出す（ADR-0092）。
fn index() -> Result<ExitCode> {
    let Some(written) = write_indexes(&repo_root()?, None)? else {
        eprintln!(
            "design-records: docs/index.md has no `okf_version` in its front matter, so docs/ is not an OKF bundle"
        );
        return Ok(ExitCode::SUCCESS);
    };
    for path in written {
        println!("{path}");
    }
    Ok(ExitCode::SUCCESS)
}

/// バンドルの索引に、追跡していないものを含めて載っていない文書と下位のディレクトリの行を足し、書いた索引のパスを返す。
/// `record` を渡すと（new）、追跡していない文書は、その記録と索引（先の new が書いた、まだコミットしていない索引）だけを
/// 数える。ほかの下書きを索引に載せると、コミットしないファイルへの行を記録と一緒にコミットさせる。バンドルでなければ
/// 何も書かずに `None`。
fn write_indexes(root: &Path, record: Option<&str>) -> Result<Option<Vec<String>>> {
    // untracked なら docs/index.md 自身が入るので、空はバンドルでないときだけ
    let mut files = bundle_files(root, true)?;
    if files.is_empty() {
        return Ok(None);
    }
    if let Some(record) = record {
        let tracked: BTreeSet<String> = markdown_under(root, BUNDLE_ROOT, false)?
            .into_iter()
            .collect();
        files.retain(|path, _| {
            tracked.contains(path) || path == record || path.ends_with("/index.md")
        });
    }
    let mut written = Vec::new();
    for (path, text) in complete_indexes(&files) {
        fs::write(root.join(&path), text).map_err(|e| format!("{path}: {e}"))?;
        written.push(path);
    }
    Ok(Some(written))
}
