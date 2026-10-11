//! 番号の予約（`new`）、振り直し（`renumber`）、予約の解放（`prune`）（ADR-0018）。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use design_records::{Kind, numbers_in, renumber_refs};

use crate::git::{
    Result, fetch_origin, file_names, git, git_ok, has_origin, local_default, origin_default,
    remote_tags, repo_root, reservations, run, shared_default, tracked,
};

/// 予約をやり直す回数の上限。ほかの人の予約と続けてぶつかるのは、同時に作る人の数だけ。
const ATTEMPTS: usize = 20;

/// `kind` の使った番号: 全ブランチの履歴、作業ツリー、予約のタグ。削除した記録の番号を使い回すと、古い参照が
/// 別の記録を指してしまうので、履歴も数える。
fn used_numbers(root: &Path, kind: Kind) -> Result<BTreeSet<u32>> {
    let history = git(
        root,
        &["log", "--all", "--format=", "--name-only", "--", kind.dir()],
    )?;
    let present: Vec<String> = file_names(&root.join(kind.dir()))?
        .into_iter()
        .map(|name| format!("{}/{name}", kind.dir()))
        .collect();
    let mut used: BTreeSet<u32> = numbers_in(
        kind,
        history.lines().chain(present.iter().map(String::as_str)),
    )
    .collect();
    used.extend(
        reservations(root)?
            .into_iter()
            .filter(|r| r.kind == kind)
            .map(|r| r.number),
    );
    Ok(used)
}

/// 予約のタグを打つコミット。origin があれば origin の既定ブランチの先端（ローカルの既定ブランチに打つと、
/// まだ push していないコミットがタグと一緒に出てしまう）、無ければ既定ブランチか HEAD の先端。
fn reservation_target(root: &Path) -> Result<String> {
    let name = match shared_default(root)? {
        Some(name) => name,
        None if has_origin(root)? => {
            return Err("origin has no default branch here; run `git fetch origin` first".into());
        }
        None => "HEAD".to_string(),
    };
    git(
        root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{name}^{{commit}}"),
        ],
    )
    .map(|s| s.trim().to_string())
    .map_err(|_| format!("{name} has no commit to put the reservation tag on; make a first commit"))
}

/// `kind` の番号を予約して返す。`wanted` なら、その番号（使っていれば止める）。省けば使った番号の最大 + 1。
/// 予約は注釈つきのタグ（メッセージは `slug`）で、origin があれば push する。ローカルの `git tag` とリモートへの
/// push は、同じ名前が既にあれば拒むので、拒まれたら次の番号でやり直す。
fn reserve(root: &Path, kind: Kind, slug: &str, wanted: Option<u32>) -> Result<u32> {
    let remote = has_origin(root)?;
    // タグを打つ先は、取ってきた origin の既定ブランチで決める
    if remote {
        fetch_origin(root, true)?;
    }
    let target = reservation_target(root)?;
    for attempt in 0..ATTEMPTS {
        // やり直しでは、先に予約したほかの clone のタグだけを取ってくる
        if remote && attempt > 0 {
            fetch_origin(root, false)?;
        }
        let used = used_numbers(root, kind)?;
        let number = match wanted {
            Some(n) if used.contains(&n) => {
                return Err(format!("{} is already used", kind.reference(n)));
            }
            Some(n) => n,
            None => used.last().map_or(1, |n| n + 1),
        };
        let tag = kind.tag(number);
        let created = run(root, &["tag", "-a", &tag, "-m", slug, &target])?;
        if !created.status.success() {
            if git_ok(
                root,
                &[
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    &format!("refs/tags/{tag}"),
                ],
            )? && wanted.is_none()
            {
                // 同じ clone のほかのセッションが先に予約した
                continue;
            }
            return Err(format!(
                "cannot create tag {tag}: {}",
                String::from_utf8_lossy(&created.stderr).trim()
            ));
        }
        if !remote {
            return Ok(number);
        }
        let pushed = run(
            root,
            &["push", "--quiet", "origin", &format!("refs/tags/{tag}")],
        )?;
        if pushed.status.success() {
            return Ok(number);
        }
        git(root, &["tag", "-d", &tag])?;
        let push_error = String::from_utf8_lossy(&pushed.stderr).trim().to_string();
        let taken = remote_tags(root, &[&format!("refs/tags/{tag}")])
            .map_err(|e| format!("cannot push the reservation tag {tag}: {push_error} ({e})"))?
            .contains(&tag);
        if taken && wanted.is_none() {
            // ほかの clone が先に予約した
            continue;
        }
        return Err(format!(
            "cannot push the reservation tag {tag}: {push_error}"
        ));
    }
    Err(format!(
        "gave up reserving a number after {ATTEMPTS} attempts"
    ))
}

/// `new`: 番号を予約し、雛形を書いて、リポジトリの直下からのパスを出す。
pub fn new(kind: Kind, slug: &str, title: &str, date: &str) -> Result<ExitCode> {
    let root = repo_root()?;
    let number = reserve(&root, kind, slug, None)?;
    let path = kind.path(number, slug);
    let text = match kind {
        Kind::Adr => design_records::template(number, title, date),
        Kind::Issue => design_records::issue_template(title),
    };
    fs::create_dir_all(root.join(kind.dir())).map_err(|e| format!("{}: {e}", kind.dir()))?;
    fs::write(root.join(&path), text).map_err(|e| format!("{path}: {e}"))?;
    println!("{path}");
    Ok(ExitCode::SUCCESS)
}

/// `base` から HEAD までに足した行（ファイルごとの行番号、1 から）。
fn added_lines(root: &Path, base: &str) -> Result<BTreeMap<String, BTreeSet<usize>>> {
    // 接頭辞と改名の検出は、diff.noprefix や diff.renames の設定に左右されないよう明示する
    let diff = git(
        root,
        &[
            "diff",
            "-U0",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--find-renames",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            base,
            "HEAD",
        ],
    )?;
    let mut added: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    let mut path: Option<String> = None;
    // ファイルの見出し（`diff --git` から最初の `@@` まで）の中か。hunk の中の、足した行が `++ ` で始まる行も
    // `+++ ` で始まる
    let mut header = false;
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            header = true;
        } else if let Some(rest) = line.strip_prefix("+++ ").filter(|_| header) {
            // 空白を含むパスの後ろには TAB が付く。引用符で包んだパス（引用符、制御文字を含むもの）は読まない
            let rest = rest.strip_suffix('\t').unwrap_or(rest);
            path = match rest.strip_prefix("b/") {
                Some(p) => Some(p.to_string()),
                None if rest == "/dev/null" => None,
                None => return Err(format!("cannot read the path in the diff: {rest}")),
            };
        } else if let Some(hunk) = line.strip_prefix("@@ ") {
            header = false;
            let Some(path) = &path else { continue };
            let new_range = hunk
                .split(' ')
                .find_map(|field| field.strip_prefix('+'))
                .ok_or_else(|| format!("cannot read the hunk header: {line}"))?;
            let (start, count) = match new_range.split_once(',') {
                Some((start, count)) => (start, count),
                None => (new_range, "1"),
            };
            let parse = |s: &str| {
                s.parse::<usize>()
                    .map_err(|_| format!("cannot read the hunk header: {line}"))
            };
            let (start, count) = (parse(start)?, parse(count)?);
            added
                .entry(path.clone())
                .or_default()
                .extend(start..start + count);
        }
    }
    Ok(added)
}

/// `text` の `lines`（行番号、1 から。`None` ならすべて）の参照を付け替える。
fn rewrite(
    text: &str,
    lines: Option<&BTreeSet<usize>>,
    kind: Kind,
    old: u32,
    new: u32,
    slug: &str,
) -> String {
    text.split_inclusive('\n')
        .enumerate()
        .map(|(i, line)| {
            if lines.is_none_or(|l| l.contains(&(i + 1))) {
                renumber_refs(kind, line, old, new, slug)
            } else {
                line.to_string()
            }
        })
        .collect()
}

/// このブランチの分岐点と、比べた既定ブランチ。ローカルの既定ブランチ（/git-merge が rebase する先）と origin の
/// 既定ブランチ（PR の base）のうち、HEAD との分岐点が新しい方を使う。2 つの分岐点が祖先の関係になければ止める。
fn branch_base(root: &Path) -> Result<Option<(String, String)>> {
    let mut bases: Vec<(String, String)> = Vec::new();
    for branch in [local_default(root)?, origin_default(root)?]
        .into_iter()
        .flatten()
    {
        let base = git(root, &["merge-base", "HEAD", &branch])?
            .trim()
            .to_string();
        bases.push((base, branch));
    }
    let Some(mut newest) = bases.first().cloned() else {
        return Ok(None);
    };
    for (base, branch) in bases.into_iter().skip(1) {
        if git_ok(root, &["merge-base", "--is-ancestor", &newest.0, &base])? {
            newest = (base, branch);
        } else if !git_ok(root, &["merge-base", "--is-ancestor", &base, &newest.0])? {
            return Err(format!(
                "{} and {branch} have diverged; update the default branch first",
                newest.1
            ));
        }
    }
    Ok(Some(newest))
}

/// `renumber`: このブランチで足した `kind` の記録 `old` を、`wanted`（省けば次の番号）に振り直して 1 コミットにする。
pub fn renumber(kind: Kind, old: u32, wanted: Option<u32>) -> Result<ExitCode> {
    let root = repo_root()?;
    if !git_ok(&root, &["rev-parse", "--verify", "--quiet", "HEAD"])? {
        return Err("there is no commit to renumber".into());
    }
    if !git(&root, &["status", "--porcelain", "--untracked-files=no"])?.is_empty() {
        return Err("commit or stash the changes first; renumber makes its own commit".into());
    }
    let based = branch_base(&root)?;
    let records: Vec<String> = tracked(&root, kind.dir())?
        .into_iter()
        .filter(|p| numbers_in(kind, [p.as_str()]).eq([old]))
        .collect();
    let mut mine = Vec::new();
    for path in &records {
        let on_base = match &based {
            Some((base, _)) => git_ok(&root, &["cat-file", "-e", &format!("{base}:{path}")])?,
            None => false,
        };
        if !on_base {
            mine.push(path.clone());
        }
    }
    let record = match mine.as_slice() {
        [one] => one.clone(),
        [] if records.is_empty() => {
            return Err(format!(
                "no tracked record is numbered {}",
                kind.reference(old)
            ));
        }
        [] => {
            let shown = based.as_ref().map_or("", |(_, branch)| branch.as_str());
            return Err(format!(
                "every record numbered {} is on {shown}; the numbers of merged records are not changed",
                kind.reference(old)
            ));
        }
        _ => {
            return Err(format!(
                "more than one record numbered {} was added on this branch: {}",
                kind.reference(old),
                mine.join(", ")
            ));
        }
    };
    let name = record.rsplit('/').next().unwrap_or(&record);
    let slug = name
        .strip_suffix(".md")
        .and_then(|n| n.split_once('-'))
        .map(|(_, slug)| slug.to_string())
        .ok_or_else(|| format!("{record}: the file name is not NNNN-<slug>.md"))?;
    // 予約の前に読めることを確かめる（読めずに止まると、予約した番号が欠番になる）
    let added = match &based {
        Some((base, _)) => Some(added_lines(&root, base)?),
        None => None,
    };
    let number = reserve(&root, kind, &slug, wanted)?;
    let new_path = kind.path(number, &slug);
    if let Err(e) = move_record(
        &root,
        kind,
        &record,
        &new_path,
        &slug,
        old,
        number,
        added.as_ref(),
    ) {
        // 書き換えの前は追跡しているファイルに変更が無かったので、HEAD に戻せば元どおりになる
        git(&root, &["reset", "--quiet", "--hard", "HEAD"])?;
        return Err(format!(
            "{e}; the changes were undone and {} stays reserved but unused",
            kind.reference(number)
        ));
    }
    println!("{new_path}");
    // 旧番号をほかの記録（既定ブランチにあるもの）が使っているときだけ、予約を解放する。ほかに使う記録が無い
    // 番号の予約を消すと、ほかの clone がその番号を振る
    if records.len() > mine.len() {
        release_old(&root, kind, old, &slug).map_err(|e| {
            format!(
                "renumbered and committed as {new_path}, but the old reservation {} was not released: {e}",
                kind.tag(old)
            )
        })?;
    }
    Ok(ExitCode::SUCCESS)
}

/// 記録 `record` を `new_path` に移し、参照を `old` から `number` に付け替えて、1 つのコミットにする。記録そのものは
/// 全体を、ほかのファイルは `added` の行（`None` なら全体）を付け替える。
#[allow(clippy::too_many_arguments)]
fn move_record(
    root: &Path,
    kind: Kind,
    record: &str,
    new_path: &str,
    slug: &str,
    old: u32,
    number: u32,
    added: Option<&BTreeMap<String, BTreeSet<usize>>>,
) -> Result<()> {
    let listed = git(root, &["ls-files", "-z"])?;
    let mut changed = Vec::new();
    for path in listed.split('\0').filter(|p| !p.is_empty()) {
        // 記録そのものは、ブランチで足したものなので全体を付け替える。ほかのファイルは、足した行だけ
        // （main から来た行の同じ番号は、main の記録を指す）
        let lines = match added {
            _ if path == record => None,
            Some(added) => match added.get(path) {
                Some(lines) => Some(lines),
                None => continue,
            },
            None => None,
        };
        let file = root.join(path);
        // シンボリックリンクの差分の行はリンクの先の行ではなく、先はリポジトリの外のこともある
        if fs::symlink_metadata(&file).is_ok_and(|m| m.file_type().is_symlink()) {
            continue;
        }
        // 中身の読めないもの（消したファイル、テキストでないファイル）は参照を書けない
        let Ok(Ok(text)) = fs::read(&file).map(String::from_utf8) else {
            continue;
        };
        let new_text = rewrite(&text, lines, kind, old, number, slug);
        if new_text != text {
            fs::write(&file, new_text).map_err(|e| format!("{path}: {e}"))?;
            changed.push(path.to_string());
        }
    }
    git(root, &["mv", record, new_path])?;
    let mut add = vec!["add", "--"];
    add.extend(changed.iter().filter(|p| *p != record).map(String::as_str));
    add.push(new_path);
    git(root, &add)?;
    let message = match kind {
        Kind::Adr => format!(
            "docs: renumber {} to {}",
            kind.reference(old),
            kind.reference(number)
        ),
        Kind::Issue => format!(
            "docs: renumber issue {} to {}",
            kind.reference(old),
            kind.reference(number)
        ),
    };
    git(root, &["commit", "--quiet", "-m", &message]).map(|_| ())
}

/// 振り直した記録（slug が `slug`）の、番号 `old` の予約を解放する。`old` はほかの記録（main にあるもの）が使って
/// いるので、番号は使い回されない。残すと、check がほかの記録に予約の注意を出し続ける。
fn release_old(root: &Path, kind: Kind, old: u32, slug: &str) -> Result<()> {
    let tag = kind.tag(old);
    let mine = reservations(root)?
        .into_iter()
        .any(|r| r.name == tag && r.slug.as_deref() == Some(slug));
    if !mine {
        return Ok(());
    }
    if has_origin(root)? && remote_tags(root, &[&format!("refs/tags/{tag}")])?.contains(&tag) {
        git(
            root,
            &[
                "push",
                "--quiet",
                "origin",
                "--delete",
                &format!("refs/tags/{tag}"),
            ],
        )?;
    }
    git(root, &["tag", "-d", &tag]).map(|_| ())
}

/// `prune`: 既定ブランチ（origin があれば origin の）の履歴に記録が入った予約のタグを、ローカルと origin から消し、
/// 消したタグの名前を出す。履歴に無い記録のタグは、消すと番号が使い回されるので残す。
pub fn prune() -> Result<ExitCode> {
    let root = repo_root()?;
    let remote = has_origin(&root)?;
    if remote {
        fetch_origin(&root, true)?;
    }
    let Some(branch) = shared_default(&root)? else {
        return Err("no default branch to read the merged records from".into());
    };
    let mut args = vec!["log", branch.as_str(), "--format=", "--name-only", "--"];
    args.extend(Kind::ALL.map(Kind::dir));
    let history: BTreeSet<String> = git(&root, &args)?.lines().map(str::to_string).collect();
    let merged: Vec<String> = reservations(&root)?
        .into_iter()
        .filter(|r| {
            r.slug
                .as_ref()
                .is_some_and(|slug| history.contains(&r.kind.path(r.number, slug)))
        })
        .map(|r| r.name)
        .collect();
    if merged.is_empty() {
        return Ok(ExitCode::SUCCESS);
    }
    if remote {
        let on_remote = remote_tags(&root, &["refs/tags/adr-*", "refs/tags/issue-*"])?;
        let refs: Vec<String> = merged
            .iter()
            .filter(|tag| on_remote.contains(*tag))
            .map(|tag| format!("refs/tags/{tag}"))
            .collect();
        if !refs.is_empty() {
            let mut args = vec!["push", "--quiet", "origin", "--delete"];
            args.extend(refs.iter().map(String::as_str));
            git(&root, &args)?;
        }
    }
    let mut args = vec!["tag", "-d"];
    args.extend(merged.iter().map(String::as_str));
    git(&root, &args)?;
    for tag in &merged {
        println!("{tag}");
    }
    Ok(ExitCode::SUCCESS)
}
