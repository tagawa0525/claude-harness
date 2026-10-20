//! 番号の予約（`new`）、振り直し（`renumber`）、予約の解放（`prune`）（ADR-0018）。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use design_records::{Kind, numbers_in, record_name, renumber_refs};

use crate::git::{
    Result, fetch_origin, file_names, git, git_ok, has_origin, local_default, lock, remote_tags,
    repo_root, reservations, run, tracked,
};

/// 予約をやり直す回数の上限。ほかの人の予約と続けてぶつかるのは、同時に作る人の数だけ。
const ATTEMPTS: usize = 20;

/// `kind` の使った番号: 全ブランチの履歴、作業ツリー、予約のタグ。削除した記録の番号を使い回すと、古い参照が
/// 別の記録を指してしまうので、履歴も数える。
fn used_numbers(root: &Path, kind: Kind) -> Result<BTreeSet<u32>> {
    // 浅い clone の履歴は切った先を返さない。マージした記録の予約は prune で消えるので、切った先で消した記録の
    // 番号は、履歴にもタグにも残らない
    if git(root, &["rev-parse", "--is-shallow-repository"])?.trim() == "true" {
        return Err(
            "the repository is a shallow clone, whose history may hide the numbers of deleted records; \
             run `git fetch --unshallow` first"
                .into(),
        );
    }
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

/// 予約のタグを打つコミット。origin があれば、取ってきた origin の既定ブランチ（`fetched`）の先端（ローカルの
/// 既定ブランチに打つと、まだ push していないコミットがタグと一緒に出てしまう。手元の古い `origin/*` には落ちない）、
/// 無ければ既定ブランチか HEAD の先端。
fn reservation_target(root: &Path, remote: bool, fetched: Option<String>) -> Result<String> {
    let name = match fetched {
        Some(name) => name,
        None if remote => {
            return Err(
                "origin has no default branch (origin/HEAD, main or master) to put the \
                 reservation tag on; push the default branch with `git push -u origin main`, \
                 or set origin/HEAD with `git remote set-head origin -a`"
                    .into(),
            );
        }
        None => local_default(root)?.unwrap_or_else(|| "HEAD".to_string()),
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
    let _lock = lock(root)?;
    let remote = has_origin(root)?;
    for _ in 0..ATTEMPTS {
        // 試すたびに、タグと既定ブランチを取ってくる。既定ブランチも取るのは、拒まれた後に相手がマージを push して
        // 予約を解放していると、タグだけではその番号が空いて見えるため。タグを打つ先も、そのたびに取ってきた
        // origin の既定ブランチの先端にする（やり直しの間に進んでいることがある）
        let fetched = if remote { fetch_origin(root)? } else { None };
        let target = reservation_target(root, remote, fetched)?;
        let used = used_numbers(root, kind)?;
        let number = match wanted {
            Some(n) if used.contains(&n) => {
                return Err(format!("{} is already used", kind.reference(n)));
            }
            Some(n) => n,
            None => used.last().map_or(1, |n| n + 1),
        };
        // 5 桁の番号は、タグにも履歴にも番号として数えない（使い回しうる）
        if number > 9999 {
            return Err(format!(
                "{} has more than four digits; the numbers of this kind are used up",
                kind.reference(number)
            ));
        }
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

/// `new`: 番号を予約し、テンプレートを書いて、リポジトリの直下からのパスを出す。
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
    // hunk の中の、次の行の新しい側の行番号。足した行は hunk の見出しの範囲でなく `+` の行で数える
    // （diff.interHunkContext で hunk がまとまると、範囲に変えていない行が入る）
    let mut next = 0;
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            header = true;
        } else if header {
            if let Some(rest) = line.strip_prefix("+++ ") {
                // 空白を含むパスの後ろには TAB が付く。引用符で包んだパス（引用符、制御文字を含むもの）は読まない
                let rest = rest.strip_suffix('\t').unwrap_or(rest);
                path = match rest.strip_prefix("b/") {
                    Some(p) => Some(p.to_string()),
                    None if rest == "/dev/null" => None,
                    None => return Err(format!("cannot read the path in the diff: {rest}")),
                };
            } else if line.starts_with("@@ ") {
                header = false;
            }
        }
        if let Some(hunk) = line.strip_prefix("@@ ").filter(|_| !header) {
            let new_range = hunk
                .split(' ')
                .find_map(|field| field.strip_prefix('+'))
                .ok_or_else(|| format!("cannot read the hunk header: {line}"))?;
            let start = new_range
                .split_once(',')
                .map_or(new_range, |(start, _)| start);
            next = start
                .parse::<usize>()
                .map_err(|_| format!("cannot read the hunk header: {line}"))?;
        } else if !header && let Some(path) = &path {
            if line.starts_with('+') {
                added.entry(path.clone()).or_default().insert(next);
                next += 1;
            } else if line.starts_with(' ') || line.is_empty() {
                // diff.suppressBlankEmpty では、空の文脈行が空白なしで出る
                next += 1;
            }
        }
    }
    Ok(added)
}

/// `text` の `lines`（行番号、1 から。`None` ならすべて）の参照を付け替える。
/// `path` は書き換える文書のパス（リポジトリの直下から）。ファイル名の参照は、文書のディレクトリから解く。
fn rewrite(
    text: &str,
    path: &str,
    lines: Option<&BTreeSet<usize>>,
    kind: Kind,
    old: u32,
    new: u32,
    slug: &str,
) -> String {
    let doc_dir = Path::new(path).parent().unwrap_or(Path::new(""));
    text.split_inclusive('\n')
        .enumerate()
        .map(|(i, line)| {
            if lines.is_none_or(|l| l.contains(&(i + 1))) {
                renumber_refs(kind, line, old, new, slug, doc_dir)
            } else {
                line.to_string()
            }
        })
        .collect()
}

/// 既定ブランチとして読むブランチ。origin があれば、origin の既定ブランチを取ってきて、その `origin/<名前>`
/// （手元の origin/HEAD の symref には頼らない。origin に既定ブランチが無ければ `None`）。無ければローカルの既定
/// ブランチ。origin のタグも取ってくるので、呼ぶ側が [`lock`] を持つ。
fn shared_branch(root: &Path) -> Result<Option<String>> {
    if has_origin(root)? {
        fetch_origin(root)
    } else {
        local_default(root)
    }
}

/// このブランチの分岐点と、比べた既定ブランチ。ローカルの既定ブランチ（/git-merge が rebase する先）と origin の
/// 既定ブランチ（`origin_branch`。PR の base）のうち、HEAD との分岐点が新しい方を使う。共通の祖先の無いブランチ
/// は比べない（どちらとも無ければ基準なし）。2 つの分岐点が祖先の関係になければ止める。
fn branch_base(root: &Path, origin_branch: Option<&str>) -> Result<Option<(String, String)>> {
    let mut bases: Vec<(String, String)> = Vec::new();
    for branch in [local_default(root)?, origin_branch.map(str::to_string)]
        .into_iter()
        .flatten()
    {
        let found = run(root, &["merge-base", "HEAD", &branch])?;
        if found.status.success() {
            let base = String::from_utf8_lossy(&found.stdout).trim().to_string();
            bases.push((base, branch));
        }
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
    // 追跡していないファイルも見る（移す先にあると、予約の後で git mv が失敗する）。submodule の中の変更だけは
    // 見ない（renumber は submodule の中に触れない）。submodule の commit の違いは見る（ステージしてあれば
    // renumber のコミットに入り、失敗して戻すときの reset で失う）
    if !git(
        &root,
        &[
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--ignore-submodules=dirty",
        ],
    )?
    .is_empty()
    {
        return Err("commit or stash the changes first; renumber makes its own commit".into());
    }
    // 既定ブランチ（origin があれば取ってきた origin の）。reserve も lock を取るので、ここで持ち続けない
    let shared = {
        let _lock = lock(&root)?;
        shared_branch(&root)?
    };
    let origin_branch = if has_origin(&root)? {
        shared.as_deref()
    } else {
        None
    };
    let based = branch_base(&root, origin_branch)?;
    let records: Vec<String> = tracked(&root, kind.dir())?
        .into_iter()
        .filter(|p| record_name(kind, p).is_some_and(|(n, _)| n == old))
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
    let slug = record_name(kind, &record)
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
        // 書き換えの前は追跡しているファイルに変更が無かったので、HEAD に戻せば元どおりになる。submodule の中の
        // 変更は残す（submodule.recurse の設定でも、submodule には入らない）
        git(
            &root,
            &[
                "reset",
                "--quiet",
                "--hard",
                "--no-recurse-submodules",
                "HEAD",
            ],
        )?;
        return Err(format!(
            "{e}; the changes were undone and {} stays reserved but unused",
            kind.reference(number)
        ));
    }
    println!("{new_path}");
    // 旧番号をほかの記録が既定ブランチ（origin があれば origin の）で使っているときだけ、予約を解放する。ほかの
    // clone から見て使う記録が無い番号の予約を消すと、その番号を振られる
    if used_on(&root, shared.as_deref(), kind, old)? {
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
    // 足した行の無い（基準の無い）ときは、旧番号の参照か記録のファイル名を含むファイルだけを読む（大きな
    // リポジトリで、追跡しているファイルをすべて読まない）。git grep は見つからないと 1 で終わり、読めない
    // ファイルは標準エラーに出すだけなので、標準エラーに何かあれば止める（参照を見落としたまま進めない）。
    // 色の設定でファイル名が飾られないようにし、submodule の中は探さない。テキストかは後で読んで決める
    // （-I は git がバイナリとみなす UTF-8 のファイルも外す）
    let mentions: Option<BTreeSet<String>> = match added {
        Some(_) => None,
        None => {
            let reference = kind.reference(old);
            let file = format!("-{slug}.md");
            let found = run(
                root,
                &[
                    "grep",
                    "--no-color",
                    "--no-recurse-submodules",
                    "-lz",
                    "-F",
                    "-e",
                    &reference,
                    "-e",
                    &file,
                    "--",
                    ".",
                ],
            )?;
            let stderr = String::from_utf8_lossy(&found.stderr);
            if !stderr.trim().is_empty() {
                return Err(format!("git grep failed: {}", stderr.trim()));
            }
            match found.status.code() {
                Some(0) => Some(
                    String::from_utf8_lossy(&found.stdout)
                        .split('\0')
                        .filter(|p| !p.is_empty())
                        .map(str::to_string)
                        .collect(),
                ),
                Some(1) => Some(BTreeSet::new()),
                _ => {
                    return Err(format!(
                        "git grep failed: {}",
                        String::from_utf8_lossy(&found.stderr).trim()
                    ));
                }
            }
        }
    };
    let mut changed = Vec::new();
    for path in listed.split('\0').filter(|p| !p.is_empty()) {
        if mentions
            .as_ref()
            .is_some_and(|m| path != record && !m.contains(path))
        {
            continue;
        }
        // 足した行だけを付け替える（main から来た行の同じ番号は、main の記録を指す）
        let mut lines = match added {
            Some(added) => match added.get(path) {
                Some(lines) => Some(lines.clone()),
                None if path == record => Some(BTreeSet::new()),
                None => continue,
            },
            None => None,
        };
        let file = root.join(path);
        // symlinkの差分の行はリンクの先の行ではなく、先はリポジトリの外のこともある
        // submodule（gitlink）の項目はディレクトリで、参照を書くファイルではない
        if fs::symlink_metadata(&file).is_ok_and(|m| m.file_type().is_symlink() || m.is_dir()) {
            continue;
        }
        // 消したファイルとテキストでないファイルは参照を書けないので飛ばす。ほかの読み込みの失敗は止める
        let bytes = match fs::read(&file) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("{path}: {e}")),
        };
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        if path == record {
            lines = Some(record_lines(&text, lines));
        }
        let new_text = rewrite(&text, path, lines.as_ref(), kind, old, number, slug);
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

/// `branch`（`None` なら無い）の先端に、`kind` の番号 `number` の記録があるか。
fn used_on(root: &Path, branch: Option<&str>, kind: Kind, number: u32) -> Result<bool> {
    let Some(branch) = branch else {
        return Ok(false);
    };
    let listed = git(
        root,
        &[
            "ls-tree",
            "-z",
            "--name-only",
            branch,
            "--",
            &format!("{}/", kind.dir()),
        ],
    )?;
    Ok(listed
        .split('\0')
        .any(|path| record_name(kind, path).is_some_and(|(n, _)| n == number)))
}

/// 振り直す記録 `text` のうち、付け替える行。`added`（`None` ならすべて）からフロントマターを除き、最初の `# ` の見出し
/// （表題）を足す。記録のフロントマターの参照（`requires`、`blocked-by` など）は自分を指さないので、同じ番号は既定
/// ブランチの記録を指す。表題は、改名で足した行に入らなくても自分の番号を書く。
fn record_lines(text: &str, added: Option<BTreeSet<usize>>) -> BTreeSet<usize> {
    let numbered: Vec<(usize, &str)> = text.lines().enumerate().map(|(i, l)| (i + 1, l)).collect();
    let front_end = match numbered.first() {
        Some((_, "---")) => numbered
            .iter()
            .skip(1)
            .find(|(_, l)| *l == "---")
            .map_or(0, |(n, _)| *n),
        _ => 0,
    };
    let mut lines: BTreeSet<usize> = match added {
        Some(added) => added,
        None => numbered.iter().map(|(n, _)| *n).collect(),
    };
    lines.retain(|n| *n > front_end);
    if let Some((n, _)) = numbered
        .iter()
        .find(|(n, l)| *n > front_end && l.starts_with("# "))
    {
        lines.insert(*n);
    }
    lines
}

/// 振り直した記録（slug が `slug`）の、番号 `old` の予約を解放する。`old` はほかの記録（main にあるもの）が使って
/// いるので、番号は使い回されない。残すと、check がほかの記録に予約の注意を出し続ける。
fn release_old(root: &Path, kind: Kind, old: u32, slug: &str) -> Result<()> {
    let _lock = lock(root)?;
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
    let _lock = lock(&root)?;
    let remote = has_origin(&root)?;
    let Some(branch) = shared_branch(&root)? else {
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
        for tag in merged.iter().filter(|tag| on_remote.contains(*tag)) {
            let full = format!("refs/tags/{tag}");
            // タグごとに消す。一覧を取った後にほかの clone が消していれば、解放は済んでいる
            let deleted = run(&root, &["push", "--quiet", "origin", "--delete", &full])?;
            if !deleted.status.success() && remote_tags(&root, &[&full])?.contains(tag) {
                return Err(format!(
                    "cannot delete the reservation tag {tag} on origin: {}",
                    String::from_utf8_lossy(&deleted.stderr).trim()
                ));
            }
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
