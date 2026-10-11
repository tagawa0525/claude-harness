//! CLI が使う git の操作（リポジトリの直下、追跡しているファイル、既定ブランチ、予約のタグ）。

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use design_records::{Kind, parse_tag};

pub type Result<T> = std::result::Result<T, String>;

/// `git args…` を `dir` で走らせた結果（失敗も含む）。
pub fn run(dir: &Path, args: &[&str]) -> Result<Output> {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        // 日本語のファイル名を引用符と 8 進数で包ませない（番号を読むため）
        .args(["-c", "core.quotePath=false"])
        .args(args)
        .output()
        .map_err(|e| format!("cannot run git (is git installed?): {e}"))
}

/// `git args…` を `dir` で走らせ、成功なら標準出力を返す。
pub fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let output = run(dir, args)?;
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
pub fn git_ok(dir: &Path, args: &[&str]) -> Result<bool> {
    run(dir, args).map(|o| o.status.success())
}

/// 今いるリポジトリの直下。
pub fn repo_root() -> Result<PathBuf> {
    let top = git(Path::new("."), &["rev-parse", "--show-toplevel"])?;
    Ok(top.trim_end_matches('\n').into())
}

/// `dir` の直下のファイル名（ディレクトリが無ければ空）。
pub fn file_names(dir: &Path) -> Result<Vec<String>> {
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

/// `dir` の直下の、追跡している（ステージ済みを含む）.md のうち、作業ツリーにあるもの。prek は追跡していない
/// ファイルを退避しないので、作業ツリーをそのまま読むと、コミットに入らない下書きで結果が変わる。
pub fn tracked(root: &Path, dir: &str) -> Result<Vec<String>> {
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

/// `refs/remotes/origin/` の既定ブランチ（`origin/HEAD`、`origin/main`、`origin/master` の順）。
pub fn origin_default(root: &Path) -> Result<Option<String>> {
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
    for name in ["main", "master"] {
        let full = format!("refs/remotes/origin/{name}");
        if git_ok(root, &["show-ref", "--verify", "--quiet", &full])? {
            return Ok(Some(format!("origin/{name}")));
        }
    }
    Ok(None)
}

/// ローカルの既定ブランチ（`main`、`master` の順）。
pub fn local_default(root: &Path) -> Result<Option<String>> {
    for name in ["main", "master"] {
        let full = format!("refs/heads/{name}");
        if git_ok(root, &["show-ref", "--verify", "--quiet", &full])? {
            return Ok(Some(name.to_string()));
        }
    }
    Ok(None)
}

/// 既定ブランチ。.claude/scripts/branch-topics.sh、worktree-add.sh、claude-hooks（git.rs）と同じ順で探す。
pub fn default_branch(root: &Path) -> Result<Option<String>> {
    if let Some(name) = origin_default(root)? {
        return Ok(Some(name));
    }
    local_default(root)
}

/// 予約と解放で見る既定ブランチ。origin があれば origin の既定ブランチだけを使う（ローカルの既定ブランチの
/// まだ push していないコミットは、ほかの clone から見えない。ADR-0018）。
pub fn shared_default(root: &Path) -> Result<Option<String>> {
    if has_origin(root)? {
        origin_default(root)
    } else {
        default_branch(root)
    }
}

/// 番号の予約と解放の、clone の中での排他。fetch（`--prune` で手元のタグを消す）、タグの作成、push を、同じ clone
/// のほかのセッションと重ねない（重なると、push する前の予約を fetch が消し、同じ番号のタグが打ち直される）。
/// 共有する git のディレクトリ（worktree の間で共通）のファイルを flock し、返したファイルを落とすか、プロセスが
/// 終わると解ける。
pub fn lock(root: &Path) -> Result<fs::File> {
    let common = git(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let path = Path::new(common.trim()).join("design-records.lock");
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    file.lock()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(file)
}

/// リモート origin があるか。
pub fn has_origin(root: &Path) -> Result<bool> {
    git_ok(root, &["remote", "get-url", "origin"])
}

/// origin から取ってくる ref。予約のタグはリモートを正とし、同じ名前の手元のタグ（push の前に止まった予約）を
/// 上書きする（`+`）。ブランチも取ってくる: `prune` が予約を解放した後の番号は、origin の既定ブランチの履歴に
/// しか残らない。
const FETCH_REFSPECS: [&str; 3] = [
    "+refs/heads/*:refs/remotes/origin/*",
    "+refs/tags/adr-*:refs/tags/adr-*",
    "+refs/tags/issue-*:refs/tags/issue-*",
];

/// origin の予約のタグを取ってくる。`branches` なら、ブランチも取ってくる。origin で消えたタグ（ほかの clone の
/// `prune` や `renumber` が解放したもの）は手元からも消す（`--prune` は refspec の行き先の ref だけを消す）。
pub fn fetch_origin(root: &Path, branches: bool) -> Result<()> {
    let mut args = vec!["fetch", "--quiet", "--prune", "--no-tags", "origin"];
    args.extend(FETCH_REFSPECS.iter().skip(usize::from(!branches)));
    git(root, &args).map(|_| ())
}

/// origin のタグのうち `patterns`（ls-remote のパターン）に合うものの名前（`refs/tags/` の後ろ）。
pub fn remote_tags(root: &Path, patterns: &[&str]) -> Result<BTreeSet<String>> {
    let mut args = vec!["ls-remote", "--tags", "--refs", "origin"];
    args.extend(patterns);
    Ok(git(root, &args)?
        .lines()
        .filter_map(|line| line.split('\t').nth(1)?.strip_prefix("refs/tags/"))
        .map(str::to_string)
        .collect())
}

/// 予約のタグ。
pub struct Reservation {
    pub name: String,
    pub kind: Kind,
    pub number: u32,
    /// 注釈つきのタグのメッセージ（予約した記録の slug）。軽量のタグなら `None`（番号だけを予約する）。
    pub slug: Option<String>,
}

/// ローカルの予約のタグ。
pub fn reservations(root: &Path) -> Result<Vec<Reservation>> {
    let listed = git(
        root,
        &[
            "for-each-ref",
            "--format=%(refname)%00%(objecttype)%00%(contents:subject)",
            "refs/tags/adr-*",
            "refs/tags/issue-*",
        ],
    )?;
    Ok(listed
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\0');
            let name = fields.next()?.strip_prefix("refs/tags/")?;
            let (kind, number) = parse_tag(name)?;
            let annotated = fields.next() == Some("tag");
            let slug = fields
                .next()
                .filter(|s| annotated && !s.is_empty())
                .map(str::to_string);
            Some(Reservation {
                name: name.to_string(),
                kind,
                number,
                slug,
            })
        })
        .collect())
}
