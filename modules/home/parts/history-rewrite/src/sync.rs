//! sync-worktrees: run の結果の ref を元のリポジトリに移した後、各 worktree の HEAD、index、作業ツリーを新しい履歴に
//! 合わせる（ADR-0045 の「## 変更」の 2026-11-11、ADR-0088）。使い方と出力の形は main.rs の冒頭。
//!
//! 未コミットの変更は、ref を移す前の HEAD と比べて見つける（移した後の HEAD と比べると、書き換えたファイルがすべて
//! 変更に見える）。機械で合わせられない worktree は、何も変えずに人に返す（Leave）。書く前にすべてを確かめ、
//! commit-refs remap を最初の書き込みにする（remap は止まるとき何も書かない）。

use crate::run::{CommitMap, read_map};
use crate::{Stop, Transformer, Words};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// この worktree は何も変えずに残す理由（人が見て合わせる）
#[derive(Debug)]
struct Leave(String);

/// 合わせ方を決める段の止まり方
enum Halt {
    Leave(Leave),
    Stop(Stop),
}

impl From<Stop> for Halt {
    fn from(e: Stop) -> Halt {
        Halt::Stop(e)
    }
}

impl From<Leave> for Halt {
    fn from(e: Leave) -> Halt {
        Halt::Leave(e)
    }
}

/// 進行中の操作の印（rev-parse --git-path で worktree ごとの場所を引く）。残したまま HEAD を移すと、印が古い履歴の
/// コミットを指したまま、次のコミットやマージが古い履歴を親に持ち込む
const OPERATIONS: [&str; 8] = [
    "MERGE_HEAD",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
    "REBASE_HEAD",
    "rebase-merge",
    "rebase-apply",
    "sequencer",
    "BISECT_LOG",
];

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn spawn_git(
    w: &Path,
    args: &[&str],
    env: &[(&str, &str)],
    input: Option<&[u8]>,
) -> Result<Output, Stop> {
    let mut command = Command::new("git");
    command.arg("-C").arg(w).args(args);
    for (k, v) in env {
        command.env(k, v);
    }
    let mut child = command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Stop(format!("git を起動できない: {e}")))?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        // git が入力を読む前に終わったら（hook や index の誤り）書けないが、理由は git の標準エラーと終了コードにあるので、
        // 待って受ける
        let _ = stdin.write_all(input);
    }
    child
        .wait_with_output()
        .map_err(|e| Stop(format!("git {} を待てない: {e}", args.join(" "))))
}

/// `git -C w <args>` の標準出力。失敗すれば、引数と終了コードと標準エラーを添えて止める
fn git(
    w: &Path,
    args: &[&str],
    env: &[(&str, &str)],
    input: Option<&[u8]>,
) -> Result<Vec<u8>, Stop> {
    let out = spawn_git(w, args, env, input)?;
    if !out.status.success() {
        return Err(Stop(format!(
            "git {}（終了コード {}）: {}",
            args.join(" "),
            out.status
                .code()
                .map_or("なし".to_string(), |c| c.to_string()),
            text(&out.stderr).trim()
        )));
    }
    Ok(out.stdout)
}

fn plain(w: &Path, args: &[&str]) -> Result<Vec<u8>, Stop> {
    git(w, args, &[], None)
}

/// worktree の中のパス p（git が出すバイト列）
fn path_in(w: &Path, p: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        w.join(std::ffi::OsStr::from_bytes(p))
    }
    #[cfg(not(unix))]
    {
        w.join(String::from_utf8_lossy(p).as_ref())
    }
}

/// `ls-files -s -z` と `ls-tree -r -z` の行を、パスから「モードと番号」への表にする。ls-files の段が 0 でない項
/// （衝突の項）があれば残す
fn entries(raw: &[u8]) -> Result<HashMap<Vec<u8>, String>, Leave> {
    let mut out = HashMap::new();
    for line in raw.split(|b| *b == 0).filter(|l| !l.is_empty()) {
        let Some(tab) = line.iter().position(|b| *b == b'\t') else {
            continue;
        };
        let (head, path) = (text(&line[..tab]), line[tab + 1..].to_vec());
        let fields: Vec<&str> = head.split_whitespace().collect();
        // ls-files -s は「モード 番号 段」、ls-tree は「モード 種類 番号」
        let (mode, oid) = if fields.len() == 3 && matches!(fields[1], "blob" | "commit" | "tree") {
            (fields[0], fields[2])
        } else {
            if fields.get(2) != Some(&"0") {
                return Err(Leave(format!("衝突の途中（{}）", text(&path))));
            }
            (fields[0], fields[1])
        };
        out.insert(path, format!("{mode} {oid}"));
    }
    Ok(out)
}

/// old から new で中身の変わったパス。パスの増減と種類の変わったものがあれば残す
fn rewritten_paths(w: &Path, old: &str, new: &str) -> Result<Vec<Vec<u8>>, Halt> {
    let raw = plain(w, &["diff-tree", "-r", "-z", "--no-renames", old, new])?;
    let fields: Vec<&[u8]> = raw.split(|b| *b == 0).collect();
    let mut paths = Vec::new();
    for pair in fields.chunks(2) {
        let [status_line, path] = pair else { continue };
        if status_line.is_empty() {
            continue;
        }
        let status_line = text(status_line);
        let parts: Vec<&str> = status_line
            .trim_start_matches(':')
            .split_whitespace()
            .collect();
        let [mode_old, mode_new, _, _, status] = parts[..] else {
            return Err(Stop(format!("git diff-tree の知らない行: {status_line}")).into());
        };
        if status != "M" || mode_old != mode_new {
            return Err(Leave(format!("書き換えが {} を {status} にした", text(path))).into());
        }
        paths.push(path.to_vec());
    }
    Ok(paths)
}

/// `-z --name-only` の出力の、パスの集まり
fn names(raw: &[u8]) -> HashSet<Vec<u8>> {
    raw.split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(<[u8]>::to_vec)
        .collect()
}

fn joined(paths: impl IntoIterator<Item = Vec<u8>>) -> String {
    let set: BTreeSet<String> = paths.into_iter().map(|p| text(&p)).collect();
    set.into_iter().collect::<Vec<_>>().join(", ")
}

fn paths_input<'a>(paths: impl IntoIterator<Item = &'a Vec<u8>>) -> Vec<u8> {
    let mut out = Vec::new();
    for p in paths {
        out.extend_from_slice(p);
        out.push(0);
    }
    out
}

/// ref を移す前の HEAD と比べた、1 つの worktree の未コミットの変更（読むだけで作る）
struct Plan {
    old: String,
    new: String,
    /// detached HEAD を old から new に移す
    move_head: bool,
    rewritten: Vec<Vec<u8>>,
    index: HashMap<Vec<u8>, String>,
    staged: HashSet<Vec<u8>>,
    unstaged: HashSet<Vec<u8>>,
}

/// 合わせ方を決める。合わせられなければ Leave。worktree には index の stat の更新だけを書く
fn plan(w: &Path, old: &str, mapping: &CommitMap) -> Result<Plan, Halt> {
    let Some(new) = mapping.get(old) else {
        return Err(Leave(format!(
            "{old} が書き換えに入っていない（1 でブランチを付けてやり直す）"
        ))
        .into());
    };
    let head = text(&plain(w, &["rev-parse", "HEAD"])?).trim().to_string();
    let detached = !spawn_git(w, &["symbolic-ref", "-q", "HEAD"], &[], None)?
        .status
        .success();
    if head != *new && !(detached && head == old) {
        return Err(Leave(format!("HEAD が {new} でない（ref を移してから）")).into());
    }
    let mut args = vec!["rev-parse"];
    for name in OPERATIONS {
        args.extend(["--git-path", name]);
    }
    let marks = text(&plain(w, &args)?);
    for (name, mark) in OPERATIONS.iter().zip(marks.lines()) {
        if std::fs::symlink_metadata(w.join(mark)).is_ok() {
            return Err(Leave(format!("操作の途中（{name}）")).into());
        }
    }
    let rewritten = rewritten_paths(w, old, new)?;

    // 衝突の項は先に見る（refresh は衝突の項があると needs merge で失敗し、理由を取り違える）
    let index = entries(&plain(w, &["ls-files", "-s", "-z"])?)?;
    let tree = entries(&plain(w, &["ls-tree", "-r", "-z", "--full-tree", old])?)?;

    // 未コミットの変更は、ref を移す前の HEAD（old）と比べる。index の stat を更新してから作業ツリーと比べる。
    // 更新できない（index.lock を持たれている）なら、後の index の書き込みも失敗するので、何も変えずに残す
    let refresh = spawn_git(w, &["update-index", "-q", "--refresh"], &[], None)?;
    if !refresh.status.success() {
        return Err(Leave(format!(
            "index を更新できない（終了コード {}。index.lock を持つプロセスが無いか確かめる）{}",
            refresh
                .status
                .code()
                .map_or("なし".to_string(), |c| c.to_string()),
            text(&refresh.stderr).trim()
        ))
        .into());
    }
    // assume-unchanged と skip-worktree の印のあるパスは、diff-files が作業ツリーを見ないので、手元の編集を見落とす
    // （ls-files -v の小文字の印と S）
    let flagged: Vec<String> = plain(w, &["ls-files", "-v", "-z"])?
        .split(|b| *b == 0)
        .filter(|l| !l.is_empty() && (l[0].is_ascii_lowercase() || l[0] == b'S'))
        .map(|l| text(&l[2.min(l.len())..]))
        .collect();
    if !flagged.is_empty() {
        return Err(Leave(format!(
            "assume-unchanged か skip-worktree の印のあるパス: {}",
            flagged.join(", ")
        ))
        .into());
    }
    let unstaged = names(&plain(w, &["diff-files", "-z", "--name-only"])?);
    // git add -N のパスは、index に空の項があるだけで中身はステージしていない。--ita-invisible-in-index は
    // その項を無いものとして比べる（diff-files の状態は、ファイルを消すと A から D に変わるので、見分けに使えない）
    let staged = names(&plain(
        w,
        &[
            "diff-index",
            "--cached",
            "--no-renames",
            "--ita-invisible-in-index",
            "-z",
            "--name-only",
            old,
        ],
    )?);
    let intent: HashSet<Vec<u8>> = names(&plain(
        w,
        &[
            "diff-index",
            "--cached",
            "--no-renames",
            "-z",
            "--name-only",
            old,
        ],
    )?)
    .difference(&staged)
    .cloned()
    .collect();
    let readded: Vec<Vec<u8>> = intent
        .iter()
        .filter(|p| tree.contains_key(*p))
        .cloned()
        .collect();
    if !readded.is_empty() {
        return Err(Leave(format!(
            "追跡していたパスを git add -N で足し直した: {}",
            joined(readded)
        ))
        .into());
    }
    let partly: Vec<Vec<u8>> = staged.intersection(&unstaged).cloned().collect();
    if !partly.is_empty() {
        // ステージし直すと、ステージしていなかった部分も入る
        return Err(Leave(format!(
            "ステージした後に変えたパス（部分のステージ）: {}",
            joined(partly)
        ))
        .into());
    }
    Ok(Plan {
        old: old.to_string(),
        new: new.clone(),
        move_head: detached && head == old,
        rewritten,
        index,
        staged,
        unstaged,
    })
}

/// 手を付けた、追跡している書き換えたファイルの、変換を当てた中身。変換が止まれば Stop
fn converted(w: &Path, plan: &Plan, t: &mut Transformer) -> Result<Vec<(PathBuf, Vec<u8>)>, Halt> {
    let mut contents = Vec::new();
    for p in &plan.rewritten {
        if !plan.staged.contains(p) && !plan.unstaged.contains(p) {
            continue;
        }
        // index に無いパス（git rm --cached、git rm）と、未コミットで消したファイルは、そのまま残す
        let f = path_in(w, p);
        let meta = std::fs::symlink_metadata(&f);
        if !plan.index.contains_key(p) || meta.is_err() {
            continue;
        }
        if meta.as_ref().is_ok_and(|m| !m.is_file()) {
            return Err(Leave(format!("{} がファイルでなくなった", text(p))).into());
        }
        let data =
            std::fs::read(&f).map_err(|e| Stop(format!("{}: 読めない: {e}", f.display())))?;
        let changed = t
            .blob(p, &data)
            .map_err(|e| Stop(format!("{} の変換が止まった: {e}", text(p))))?;
        if let Some(changed) = changed {
            contents.push((f, changed));
        }
    }
    Ok(contents)
}

/// f の中身を data にする。同じディレクトリの一時ファイルに書き、f の権限をコピーして rename で置き換える（f に直に
/// 書くと先に中身を消すので、書き込みの失敗で未コミットの編集が壊れる。読み出し専用のファイルも書ける）
fn replace_file(f: &Path, data: &[u8]) -> Result<(), Stop> {
    let fail = |e: std::io::Error| Stop(format!("{}: 書けない: {e}", f.display()));
    let permissions = std::fs::metadata(f).map_err(fail)?.permissions();
    let dir = f.parent().unwrap_or(Path::new("."));
    let name = f
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut k = 0u32;
    let (tmp, mut file) = loop {
        let tmp = dir.join(format!(".{name}.{}.{k}", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(file) => break (tmp, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => k += 1,
            Err(e) => return Err(fail(e)),
        }
    };
    let written = file
        .write_all(data)
        .and_then(|()| file.sync_all())
        .and_then(|()| std::fs::set_permissions(&tmp, permissions.clone()))
        .and_then(|()| replace_over(&tmp, f, &permissions));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(fail(e));
    }
    Ok(())
}

/// tmp を f の名前に置き換える。Windows は読み出し専用のファイルの上に rename できないので、置き換える間だけ f の
/// 読み出し専用を外す（置き換えた後の f は、tmp に付けた元の権限を持つ）
fn replace_over(tmp: &Path, f: &Path, permissions: &std::fs::Permissions) -> std::io::Result<()> {
    if cfg!(windows) && permissions.readonly() {
        let mut writable = permissions.clone();
        #[allow(clippy::permissions_set_readonly_false)]
        writable.set_readonly(false);
        std::fs::set_permissions(f, writable)?;
        let renamed = std::fs::rename(tmp, f);
        if renamed.is_err() {
            let _ = std::fs::set_permissions(f, permissions.clone());
        }
        return renamed;
    }
    std::fs::rename(tmp, f)
}

/// 1 つの worktree を合わせる。変更が無ければ None、あれば commit-refs remap の出力を返す。
/// 書く前に止まれば、何も変えずに Leave。書き始めた後に止まれば、worktree のパスを添えて Stop
fn sync_one(
    w: &Path,
    old: &str,
    mapping: &CommitMap,
    map_path: &Path,
    t: &mut Transformer,
) -> Result<Option<String>, Halt> {
    let before_write = |t: &mut Transformer| -> Result<(Plan, bool, Option<Output>), Halt> {
        let plan = plan(w, old, mapping)?;
        let clean = plan.staged.is_empty() && plan.unstaged.is_empty();
        if clean {
            return Ok((plan, clean, None));
        }
        converted(w, &plan, t)?; // 書く前に、変換が止まらないことを確かめる
        // 未コミットの文書の古い番号を付け直す。これを最初の書き込みにするのは、remap が止まる理由（曖昧な番号、
        // 消えたコミットを指す番号）は文書の中身によって起き、止まるときは何も書かないので、worktree を変えずに残せるため
        let remap = Command::new("commit-refs")
            .arg("remap")
            .arg(map_path)
            .current_dir(w)
            .output()
            .map_err(|e| {
                Stop(format!(
                    "commit-refs を起動できない（PATH に commit-refs が無い）: {e}"
                ))
            })?;
        let stderr = text(&remap.stderr).trim().to_string();
        if !remap.status.success() && stderr.ends_with("何も書き換えていない") {
            return Err(Leave(format!("commit-refs remap が止まった: {stderr}")).into());
        }
        Ok((plan, clean, Some(remap)))
    };
    let (plan, clean, remap) = match before_write(t) {
        Ok(found) => found,
        Err(Halt::Stop(Stop(why))) => return Err(Leave(why).into()),
        Err(leave) => return Err(leave),
    };
    let wrote = (|| -> Result<Option<String>, Halt> {
        if let Some(remap) = &remap
            && !remap.status.success()
        {
            return Err(Stop(format!(
                "commit-refs remap が書き込みの途中で止まった: {}",
                text(&remap.stderr).trim()
            ))
            .into());
        }
        apply(
            w,
            &plan,
            clean,
            t,
            remap.as_ref().map(|r| r.stdout.as_slice()),
        )
    })();
    wrote.map_err(|e| {
        let why = match e {
            Halt::Stop(Stop(why)) | Halt::Leave(Leave(why)) => why,
        };
        Stop(format!(
            "{}: 途中まで合わせて止まった（未コミットの変更は作業ツリーに残る。HEAD、index、作業ツリーの一部が新しい履歴になっている）: {why}",
            w.display()
        ))
        .into()
    })
}

fn apply(
    w: &Path,
    plan: &Plan,
    clean: bool,
    t: &mut Transformer,
    remapped: Option<&[u8]>,
) -> Result<Option<String>, Halt> {
    // GIT_LITERAL_PATHSPECS を足すのは、パスの *、? を pathspec の型として読ませないため
    let env = [("GIT_LITERAL_PATHSPECS", "1")];
    // 変換は、remap が番号を付け直した後の中身に当てる
    let contents = if clean {
        Vec::new()
    } else {
        converted(w, plan, t)?
    };
    if plan.move_head {
        plain(
            w,
            &["update-ref", "--no-deref", "HEAD", &plan.new, &plan.old],
        )?;
    }
    if clean {
        // 書き換えはパスを増やさないので（rewritten_paths）、追跡していないファイルを上書きしない
        git(w, &["reset", "-q", "--hard", "HEAD"], &env, None)?;
        return Ok(None);
    }
    let untouched: Vec<&Vec<u8>> = plan
        .rewritten
        .iter()
        .filter(|p| !plan.staged.contains(*p) && !plan.unstaged.contains(*p))
        .collect();
    let from_file = ["--pathspec-from-file=-", "--pathspec-file-nul"];
    if !untouched.is_empty() {
        // 手を付けていないファイルは、index と作業ツリーを新しい中身にする
        let args = [&["checkout", "HEAD"][..], &from_file].concat();
        git(w, &args, &env, Some(&paths_input(untouched)))?;
    }
    let index_only: Vec<&Vec<u8>> = plan
        .rewritten
        .iter()
        .filter(|p| plan.unstaged.contains(*p))
        .collect();
    if !index_only.is_empty() {
        // 変えたがステージしていないファイルの index を、新しい HEAD の中身にする（作業ツリーは変えない）
        let args = [&["reset", "-q", "HEAD"][..], &from_file].concat();
        git(w, &args, &env, Some(&paths_input(index_only)))?;
    }
    for (f, data) in &contents {
        replace_file(f, data)?;
    }
    // ステージしていたパスを、直した中身でステージし直す（部分のステージは plan で除いた）。index に無いパス
    // （git rm --cached、git rm）は足さない
    let mut restage: Vec<&Vec<u8>> = plan
        .staged
        .iter()
        .filter(|p| plan.index.contains_key(*p))
        .collect();
    restage.sort();
    if !restage.is_empty() {
        let args = [&["add", "-f"][..], &from_file].concat();
        git(w, &args, &env, Some(&paths_input(restage)))?;
    }
    Ok(Some(text(remapped.unwrap_or_default())))
}

/// git と remap の複数行の理由を、worktree ごとの 1 行の出力に収める
fn one_line(why: &str) -> String {
    why.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" / ")
}

/// worktree-state の行（worktree のパス、ref を移す前の HEAD、控えのパッチのパスをタブで区切る）
fn read_state(path: &Path) -> Result<Vec<(PathBuf, String, String)>, Stop> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Stop(format!("{}: 読めない: {e}", path.display())))?;
    let mut rows = Vec::new();
    for (k, line) in text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let [w, old, patch] = fields[..] else {
            return Err(Stop(format!(
                "{}:{}: タブで区切った 3 列でない: {line:?}",
                path.display(),
                k + 1
            )));
        };
        rows.push((PathBuf::from(w), old.to_string(), patch.to_string()));
    }
    Ok(rows)
}

/// sync-worktrees の本体。LEFT か ERROR が 1 つでもあれば false
pub fn sync_worktrees(words: &Words, workdir: &Path) -> Result<bool, Stop> {
    let mut transformer = Transformer::new(words)?;
    let workdir =
        crate::real_path(workdir).map_err(|e| Stop(format!("{}: {e}", workdir.display())))?;
    let map_path = workdir.join("commit-map");
    let mapping = read_map(&map_path)?;
    let rows = read_state(&workdir.join("worktree-state"))?;
    let mut ok = true;
    for (w, old, patch) in rows {
        let result = (|| -> Result<Option<Option<String>>, Halt> {
            if !w.is_dir() {
                return Err(Leave("ディレクトリが無い".to_string()).into());
            }
            let bare = plain(&w, &["rev-parse", "--is-bare-repository"])
                .map_err(|Stop(why)| Leave(why))?;
            if text(&bare).trim() == "true" {
                return Ok(None);
            }
            sync_one(&w, &old, &mapping, &map_path, &mut transformer).map(Some)
        })();
        match result {
            Ok(None) => println!("SKIPPED: {} (bare)", w.display()),
            Ok(Some(None)) => println!("RESET: {}", w.display()),
            Ok(Some(Some(remapped))) => {
                println!("KEPT: {} {patch}", w.display());
                print!("{remapped}");
            }
            Err(Halt::Leave(Leave(why))) => {
                println!("LEFT: {} ({})", w.display(), one_line(&why));
                ok = false;
            }
            Err(Halt::Stop(Stop(why))) => {
                eprintln!("ERROR: {}", one_line(&why));
                ok = false;
            }
        }
        std::io::stdout().flush().map_err(|e| Stop(e.to_string()))?;
    }
    Ok(ok)
}
