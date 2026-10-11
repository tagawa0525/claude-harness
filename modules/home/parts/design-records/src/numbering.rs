//! 番号の予約（`new`）、振り直し（`renumber`）、予約の解放（`prune`）（ADR-0018）。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use design_records::{Kind, numbers_in, renumber_refs};

use crate::git::{
    Result, default_branch, fetch_tags, file_names, git, git_ok, has_origin, remote_has_tag,
    repo_root, reservations, run, shared_default, tracked,
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
    let target = reservation_target(root)?;
    for _ in 0..ATTEMPTS {
        if remote {
            fetch_tags(root)?;
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
        if remote_has_tag(root, &tag)? && wanted.is_none() {
            // ほかの clone が先に予約した
            continue;
        }
        return Err(format!(
            "cannot push the reservation tag {tag}: {}",
            String::from_utf8_lossy(&pushed.stderr).trim()
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
            "--find-renames",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            base,
            "HEAD",
        ],
    )?;
    let mut added: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
    let mut path: Option<String> = None;
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("+++ ") {
            path = match rest.strip_prefix("b/") {
                Some(p) => Some(p.to_string()),
                None if rest == "/dev/null" => None,
                None => return Err(format!("cannot read the path in the diff: {rest}")),
            };
        } else if let Some(hunk) = line.strip_prefix("@@ ") {
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

/// `renumber`: このブランチで足した `kind` の記録 `old` を、`wanted`（省けば次の番号）に振り直して 1 コミットにする。
pub fn renumber(kind: Kind, old: u32, wanted: Option<u32>) -> Result<ExitCode> {
    let root = repo_root()?;
    if !git_ok(&root, &["rev-parse", "--verify", "--quiet", "HEAD"])? {
        return Err("there is no commit to renumber".into());
    }
    if !git(&root, &["status", "--porcelain", "--untracked-files=no"])?.is_empty() {
        return Err("commit or stash the changes first; renumber makes its own commit".into());
    }
    let (base, shown) = match default_branch(&root)? {
        Some(branch) => (
            Some(
                git(&root, &["merge-base", "HEAD", &branch])?
                    .trim()
                    .to_string(),
            ),
            branch,
        ),
        None => (None, String::new()),
    };
    let records: Vec<String> = tracked(&root, kind.dir())?
        .into_iter()
        .filter(|p| numbers_in(kind, [p.as_str()]).eq([old]))
        .collect();
    let mut mine = Vec::new();
    for path in &records {
        let on_base = match &base {
            Some(base) => git_ok(&root, &["cat-file", "-e", &format!("{base}:{path}")])?,
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
    let number = reserve(&root, kind, &slug, wanted)?;

    let added = match &base {
        Some(base) => Some(added_lines(&root, base)?),
        None => None,
    };
    let listed = git(&root, &["ls-files", "-z"])?;
    let mut changed = Vec::new();
    for path in listed.split('\0').filter(|p| !p.is_empty()) {
        // 記録そのものは、ブランチで足したものなので全体を付け替える。ほかのファイルは、足した行だけ
        // （main から来た行の同じ番号は、main の記録を指す）
        let lines = match &added {
            _ if path == record => None,
            Some(added) => match added.get(path) {
                Some(lines) => Some(lines),
                None => continue,
            },
            None => None,
        };
        let file = root.join(path);
        // 中身の読めないもの（シンボリックリンクの先が無いもの、テキストでないファイル）は参照を書けない
        let Ok(text) = fs::read(&file).map(String::from_utf8) else {
            continue;
        };
        let Ok(text) = text else { continue };
        let new_text = rewrite(&text, lines, kind, old, number, &slug);
        if new_text != text {
            fs::write(&file, new_text).map_err(|e| format!("{path}: {e}"))?;
            changed.push(path.to_string());
        }
    }
    let new_path = kind.path(number, &slug);
    git(&root, &["mv", &record, &new_path])?;
    let mut add = vec!["add", "--"];
    add.extend(changed.iter().filter(|p| **p != record).map(String::as_str));
    add.push(&new_path);
    git(&root, &add)?;
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
    git(&root, &["commit", "--quiet", "-m", &message])?;
    println!("{new_path}");
    Ok(ExitCode::SUCCESS)
}

/// `prune`: 既定ブランチ（origin があれば origin の）の履歴に記録が入った予約のタグを、ローカルと origin から消し、
/// 消したタグの名前を出す。履歴に無い記録のタグは、消すと番号が使い回されるので残す。
pub fn prune() -> Result<ExitCode> {
    let root = repo_root()?;
    let remote = has_origin(&root)?;
    if remote {
        fetch_tags(&root)?;
    }
    let Some(branch) = shared_default(&root)? else {
        return Err("no default branch to read the merged records from".into());
    };
    let mut args = vec!["log", branch.as_str(), "--format=", "--name-only", "--"];
    args.extend(Kind::ALL.map(Kind::dir));
    let history: BTreeSet<String> = git(&root, &args)?.lines().map(str::to_string).collect();
    for reservation in reservations(&root)? {
        let Some(slug) = &reservation.slug else {
            continue;
        };
        if !history.contains(&reservation.kind.path(reservation.number, slug)) {
            continue;
        }
        let tag = &reservation.name;
        if remote && remote_has_tag(&root, tag)? {
            git(
                &root,
                &[
                    "push",
                    "--quiet",
                    "origin",
                    "--delete",
                    &format!("refs/tags/{tag}"),
                ],
            )?;
        }
        git(&root, &["tag", "-d", tag])?;
        println!("{tag}");
    }
    Ok(ExitCode::SUCCESS)
}
