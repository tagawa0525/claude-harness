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
    // origin/HEAD の指す先が消えていれば（origin で既定ブランチの名前を変えた後など）、使わずに名前で探す
    if let Ok(head) = git(
        root,
        &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"],
    ) && git_ok(root, &["show-ref", "--verify", "--quiet", head.trim()])?
        && let Some(name) = head.trim().strip_prefix("refs/remotes/")
    {
        return Ok(Some(name.to_string()));
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

/// origin の予約のタグの refspec。リモートを正とし、同じ名前の手元のタグ（push の前に止まった予約）を上書きする
/// （`+`）。
const TAG_REFSPECS: [&str; 2] = [
    "+refs/tags/adr-*:refs/tags/adr-*",
    "+refs/tags/issue-*:refs/tags/issue-*",
];

/// origin の予約のタグを取ってくる。origin で消えたタグ（ほかの clone の `prune` や `renumber` が解放したもの）は
/// 手元からも消す（`--prune` は、同じ fetch の refspec の行き先の ref だけを消すので、タグだけで fetch する）。
/// `branch` なら、その後で origin の既定ブランチも取ってきて、`origin/<名前>` を返す（`prune` が予約を解放した後の
/// 番号は、origin の既定ブランチの履歴にしか残らない）。origin に既定ブランチが無ければ `None`。
///
/// タグを先に取る: ほかの clone はマージを push してから予約を解放するので、タグを取った後に取るブランチには、
/// そのとき消えていたタグの記録が必ず入っている。逆の順では、間にマージと解放が起きると、マージ前のブランチと
/// 消えたタグを見て番号を使い回す。
pub fn fetch_origin(root: &Path, branch: bool) -> Result<Option<String>> {
    let mut args = vec!["fetch", "--quiet", "--prune", "--no-tags", "origin"];
    args.extend(TAG_REFSPECS);
    git(root, &args)?;
    if !branch {
        return Ok(None);
    }
    let Some(name) = remote_default(root)? else {
        return Ok(None);
    };
    let refspec = format!("+refs/heads/{name}:refs/remotes/origin/{name}");
    git(root, &["fetch", "--quiet", "--no-tags", "origin", &refspec])?;
    Ok(Some(format!("origin/{name}")))
}

/// origin の既定ブランチの名前。origin の HEAD が指すブランチ、それが無ければ（HEAD が空のブランチを指す）`main`、
/// `master` のうち origin にあるもの。
fn remote_default(root: &Path) -> Result<Option<String>> {
    let listed = git(
        root,
        &[
            "ls-remote",
            "--symref",
            "origin",
            "HEAD",
            "refs/heads/main",
            "refs/heads/master",
        ],
    )?;
    let heads: BTreeSet<&str> = listed
        .lines()
        .filter(|line| !line.starts_with("ref: "))
        .filter_map(|line| line.split('\t').nth(1)?.strip_prefix("refs/heads/"))
        .collect();
    let head = listed.lines().find_map(|line| {
        line.strip_prefix("ref: refs/heads/")?
            .strip_suffix("\tHEAD")
    });
    // HEAD の指す先があるときは、HEAD のコミットの行も出る
    let resolved = listed
        .lines()
        .any(|line| !line.starts_with("ref: ") && line.ends_with("\tHEAD"));
    if let Some(head) = head.filter(|_| resolved) {
        return Ok(Some(head.to_string()));
    }
    Ok(["main", "master"]
        .into_iter()
        .find(|name| heads.contains(name))
        .map(str::to_string))
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
