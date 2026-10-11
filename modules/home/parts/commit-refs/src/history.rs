//! rewrite-history: 履歴の中の文書と本文の番号を、書き換えた後の最後の番号に付け直す（ADR-0013）。
//!
//! 1. `git fast-export --no-data` で、全てのブランチとタグから届くコミットを読む（ブロブは `git cat-file` で読む）
//! 2. 全てのコミットの本文とブロブを調べ、文書が指すコミットを求める。止める理由は、ここで全て出す
//! 3. 親と、文書が指すコミットの両方を依存として並べ（元の順をなるべく保つ）、番号を置き換えて
//!    `git fast-import` に渡す。コミットを 1 つ渡すたびに `get-mark` で最後の番号を受け取り、後の置き換えに使う
//! 4. 注釈つきのタグを `git mktag` で作り直す
//! 5. 元の全てのコミットとタグについて、書き換えた後のものが番号の置き換えだけを当てたものかを確かめる
//! 6. ブランチとタグを 1 つのトランザクションで移し、元の ref を `refs/commit-refs/original/` に残す

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use commit_refs::{Map, Target, is_full_hash};

use crate::{git, read_map};

/// fast-import がコミットを積むブランチ。トランザクションで消す
const SCRATCH: &str = "refs/commit-refs/new/stream";
const ORIGINAL_NS: &str = "refs/commit-refs/original/";

/// テキストかどうかを見る先頭のバイト数（git の xdiff-interface.c の FIRST_FEW_BYTES と同じ）
const FIRST_FEW_BYTES: usize = 8000;

/// filter-repo 2.47.0 の fep_cmd と同じ設定に、ブロブを流さない設定、署名つきのコミットの扱い、本文を元の
/// 符号化のまま渡す設定を足す（番号を置き換えると中身が変わり、元の署名は合わない）。タグは流れから読まずに
/// オブジェクトから作り直すが、fast-export はタグを流れに出すので、タグを指すタグでも止まらないよう、タグの設定も
/// 残す
const FAST_EXPORT: &[&str] = &[
    "fast-export",
    "--no-data",
    "--show-original-ids",
    "--signed-commits=strip",
    "--signed-tags=strip",
    "--tag-of-filtered-object=rewrite",
    "--fake-missing-tagger",
    "--reference-excluded-parents",
    "--use-done-feature",
    "--mark-tags",
    "--reencode=no",
];

const MODE_LINK: &str = "120000";
const MODE_GITLINK: &str = "160000";

type Errors = Vec<String>;

pub fn run(map_path: &Path) -> Result<(), Errors> {
    let text = read_map(Some(map_path))?;
    let mut map = Map::parse(&text)?;
    let top = git::toplevel().map_err(one)?;
    let map_file = map_file(&top).map_err(one)?;
    preflight(&top, &map_file).map_err(one)?;

    let all_refs = for_each_ref(&top).map_err(one)?;
    let (refs, not_rewritten) = split_refs(&top, all_refs).map_err(one)?;
    if refs.is_empty() {
        return Err(one("書き換えるブランチとタグが無い".to_string()));
    }
    let commits = rev_list(&top, &refs).map_err(one)?;
    let current: HashSet<String> = commits.iter().cloned().collect();
    // 対応表で変わった（消えた）とされる旧が今の履歴にも残るのは、一部の ref だけを書き換えた対応表。文書がその旧を
    // 指すとき、残っている旧か書き換えた後の新のどちらを指すのか決まらない
    let left: Vec<String> = map
        .targets()
        .filter(|(old, target)| **target != Target::Unchanged && current.contains(*old))
        .map(|(old, _)| old.to_string())
        .collect();
    if !left.is_empty() {
        return Err(vec![format!(
            "対応表で変わった（消えた）とされる旧が、書き換えるブランチとタグから届く（一部の ref だけを書き換えた対応表は扱わない）:\n{}",
            left.join("\n")
        )]);
    }
    map.add_unchanged(commits.iter().map(String::as_str));
    let head = head_state(&top).map_err(one)?;
    if let Head::Detached(oid) = &head
        && !current.contains(oid)
    {
        return Err(one(format!(
            "HEAD が、書き換えるブランチとタグから届かないコミット {oid} にある（ブランチに移ってから）"
        )));
    }

    let mut rewrite = Rewrite {
        top: &top,
        map: &map,
        current: &current,
        cat: CatFile::new(&top).map_err(one)?,
        hash_bytes: hash_bytes(&top).map_err(one)?,
        finals: HashMap::new(),
        tag_finals: HashMap::new(),
        refs_replaced: 0,
    };
    let result = rewrite
        .commits(&refs)
        .and_then(|()| rewrite.tags(&refs))
        .and_then(|()| rewrite.verify(&commits))
        .and_then(|()| move_refs(&top, &refs, &rewrite, &head));
    if let Err(mut errors) = result {
        if let Err(e) = delete_scratch(&top) {
            errors.push(e);
        }
        errors.push("ブランチとタグは動かしていない".to_string());
        return Err(errors);
    }

    // ref を移したら、すぐに対応表を書く（後の作業ツリーの更新が失敗しても、対応は残る）
    write_map(&map_file, &map, &rewrite.finals).map_err(|e| {
        vec![
            e,
            "ブランチとタグは移した。元の ref は refs/commit-refs/original/ にある".to_string(),
        ]
    })?;
    // 始める前に、作業ツリーに追跡しているファイルの変更が無いことを確かめた。書き換えはファイルの中身だけを
    // 変え、パスを足さないので、追跡していないファイルを上書きすることも無い
    git::git(&top, &["reset", "-q", "--hard"]).map_err(|e| {
        vec![
            e,
            "ブランチとタグは移し、対応表も書いた。作業ツリーは git reset --hard で合わせる"
                .to_string(),
        ]
    })?;

    let changed = rewrite.finals.iter().filter(|(c, f)| c != f).count();
    let shown = map_file.strip_prefix(&top).unwrap_or(&map_file);
    let mut report = format!(
        "COMMITS: {}\nCHANGED: {changed}\nREFS: {}\nMAP: {}\n",
        rewrite.finals.len(),
        rewrite.refs_replaced,
        shown.display()
    );
    for name in not_rewritten {
        report.push_str(&format!("NOT_REWRITTEN: {name}\n"));
    }
    print!("{report}");
    Ok(())
}

fn one(e: String) -> Errors {
    vec![e]
}

fn io_error(e: std::io::Error) -> Errors {
    vec![format!("git とのやりとりに失敗した: {e}")]
}

/// 書き換えた後の対応表の置き場所（linked worktree でも共通の .git の下）
fn map_file(top: &Path) -> Result<PathBuf, String> {
    let dir = git::git(
        top,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let dir = String::from_utf8_lossy(&dir)
        .trim_end_matches('\n')
        .to_string();
    Ok(Path::new(&dir).join("commit-refs").join("commit-map"))
}

/// 作業ツリーに変更が無く、ほかの worktree が無く、前の書き換えの ref と対応表が残っていないこと
fn preflight(top: &Path, map_file: &Path) -> Result<(), String> {
    let status = git::git(top, &["status", "--porcelain", "--untracked-files=no"])?;
    if !status.is_empty() {
        return Err(format!(
            "作業ツリーに変更がある（コミットするか片付けてから）:\n{}",
            String::from_utf8_lossy(&status).trim_end()
        ));
    }
    let worktrees = git::git(top, &["worktree", "list", "--porcelain"])?;
    let worktrees: Vec<String> = String::from_utf8_lossy(&worktrees)
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(str::to_string)
        .collect();
    if worktrees.len() > 1 {
        return Err(format!(
            "ほかの worktree がある（ブランチを移すと、その worktree が食い違う。取り除いてから）:\n{}",
            worktrees.join("\n")
        ));
    }
    let left = git::git(
        top,
        &["for-each-ref", "--format=%(refname)", "refs/commit-refs/"],
    )?;
    if !left.is_empty() {
        return Err(format!(
            "前の書き換えの ref が残っている（確かめて消してから）:\n{}",
            String::from_utf8_lossy(&left).trim_end()
        ));
    }
    if map_file.exists() {
        return Err(format!(
            "前の書き換えの対応表が残っている（確かめて消してから）: {}",
            map_file.display()
        ));
    }
    Ok(())
}

/// ref の名前と、指すオブジェクトとその種類
struct Ref {
    name: String,
    oid: String,
    kind: String,
    /// シンボリック ref なら、行き先の ref の名前
    symref: String,
}

fn for_each_ref(top: &Path) -> Result<Vec<Ref>, String> {
    let out = git::git(
        top,
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname) %(objecttype) %(symref)",
        ],
    )?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, ' ');
            Some(Ref {
                name: parts.next()?.to_string(),
                oid: parts.next()?.to_string(),
                kind: parts.next()?.to_string(),
                symref: parts.next().unwrap_or_default().to_string(),
            })
        })
        .collect())
}

/// 書き換える ref（コミットを指すブランチと、たどるとコミットに着くタグ）と、書き換えない ref の名前に分ける
fn split_refs(top: &Path, all: Vec<Ref>) -> Result<(Vec<Ref>, Vec<String>), String> {
    let mut rewritten = Vec::new();
    let mut others = Vec::new();
    for r in all {
        // シンボリック ref は行き先と一緒に動くので、移さない（行き先と一緒に更新すると、同じ ref を 2 回更新する
        // トランザクションになる）
        if !r.symref.is_empty() {
            continue;
        }
        let target = if r.name.starts_with("refs/heads/") {
            r.kind == "commit"
        } else if r.name.starts_with("refs/tags/") {
            match r.kind.as_str() {
                "commit" => true,
                "tag" => {
                    let peeled = format!("{}^{{commit}}", r.oid);
                    git::run_git(top, &["rev-parse", "--verify", "-q", &peeled])?
                        .status
                        .success()
                }
                _ => false,
            }
        } else {
            false
        };
        if target {
            rewritten.push(r);
        } else {
            others.push(r.name);
        }
    }
    Ok((rewritten, others))
}

/// 書き換える ref から届く全てのコミット
fn rev_list(top: &Path, refs: &[Ref]) -> Result<Vec<String>, String> {
    let out = git::git_stdin(top, &["rev-list", "--stdin"], &revisions(refs))?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .map(str::to_string)
        .collect())
}

/// rev-list と fast-export に標準入力で渡す版の指定。引数で渡すと、ref の数が多いと引数の上限を超え、ref と同じ
/// 名前のパスがあると版かパスかが曖昧になる
fn revisions(refs: &[Ref]) -> Vec<u8> {
    refs.iter()
        .flat_map(|r| format!("{}\n", r.name).into_bytes())
        .collect()
}

enum Head {
    /// ブランチを指している（ブランチと一緒に動く）
    Branch,
    /// コミットを直接指している
    Detached(String),
}

fn head_state(top: &Path) -> Result<Head, String> {
    let symbolic = git::run_git(top, &["symbolic-ref", "-q", "HEAD"])?;
    if symbolic.status.success() {
        return Ok(Head::Branch);
    }
    let oid = git::git(top, &["rev-parse", "HEAD"])?;
    Ok(Head::Detached(
        String::from_utf8_lossy(&oid).trim().to_string(),
    ))
}

/// fast-export の流れから読んだコミット
struct CommitRecord {
    /// fast-export が振った印（`:N`）。fast-import にも同じ印で渡す
    mark: String,
    /// 今の番号
    original: String,
    /// author、committer、encoding の行
    header: Vec<u8>,
    message: Vec<u8>,
    /// 親の印
    parents: Vec<String>,
    ops: Vec<FileOp>,
}

enum FileOp {
    /// `M <mode> <ブロブの番号> <パス>`
    Modify {
        mode: String,
        oid: String,
        path: Vec<u8>,
    },
    /// そのまま渡す行（D、C、R、deleteall）
    Other(Vec<u8>),
}

struct Rewrite<'a> {
    top: &'a Path,
    map: &'a Map,
    current: &'a HashSet<String>,
    cat: CatFile,
    /// オブジェクトの番号のバイト数（SHA-1 は 20、SHA-256 は 32）
    hash_bytes: usize,
    /// 今のコミット → 書き換えた後の最後のコミット
    finals: HashMap<String, String>,
    /// 今の注釈つきのタグ → 書き換えた後のタグ
    tag_finals: HashMap<String, String>,
    refs_replaced: usize,
}

impl Rewrite<'_> {
    /// 文書の語が当たった旧が、今のどのコミットか（消えたコミットなら理由を積み、None）
    fn commit_of<'m>(
        label: &str,
        token: &str,
        old: &'m str,
        target: &'m Target,
        errors: &mut Errors,
    ) -> Option<&'m str> {
        match target {
            Target::Pruned => {
                errors.push(format!(
                    "{label}: {token} は書き換えで消えたコミット {old} を指す"
                ));
                None
            }
            Target::Changed(new) => Some(new.as_str()),
            Target::Unchanged => Some(old),
        }
    }

    /// 文書が指す、今のコミット（書き換えるもの）を集める
    fn references(&self, label: &str, data: &[u8], errors: &mut Errors) -> Vec<String> {
        let mut found = Vec::new();
        self.map
            .replace_by(label, data, errors, |token, old, target, errors| {
                if let Some(commit) = Self::commit_of(label, token, old, target, errors)
                    && self.current.contains(commit)
                {
                    found.push(commit.to_string());
                }
                None
            });
        found
    }

    /// テキストの番号を、書き換えた後の最後の番号に置き換えた内容と、置き換えた語の数
    fn transform(&self, label: &str, data: &[u8], errors: &mut Errors) -> (Vec<u8>, usize) {
        self.map
            .replace_by(label, data, errors, |token, old, target, errors| {
                let commit = Self::commit_of(label, token, old, target, errors)?;
                match self.finals.get(commit) {
                    Some(last) => Some(last.clone()),
                    // 依存の順に流すので、書き換えるコミットは先に流れている
                    None if self.current.contains(commit) => {
                        errors.push(format!(
                            "{label}: {token} が指すコミット {commit} を、まだ書き換えていない"
                        ));
                        None
                    }
                    // 書き換える ref から届かないコミットは、この書き換えで変わらない
                    None => Some(commit.to_string()),
                }
            })
    }

    /// ブロブの中身を、モードに応じて置き換える（シンボリックリンクの行き先とバイナリはそのまま）
    fn transform_blob(
        &self,
        label: &str,
        mode: &str,
        data: &[u8],
        errors: &mut Errors,
    ) -> (Vec<u8>, usize) {
        if mode == MODE_LINK || is_binary(data) {
            return (data.to_vec(), 0);
        }
        self.transform(label, data, errors)
    }

    /// コミットを読み、依存の順に並べて、番号を置き換えて fast-import に渡す
    fn commits(&mut self, refs: &[Ref]) -> Result<(), Errors> {
        let records = read_commits(self.top, refs)?;
        // タグの本文の止める理由も、流す前にコミットの理由と一緒に出す
        let mut errors = Vec::new();
        let mut seen = HashSet::new();
        for r in refs {
            if r.kind == "tag" {
                self.scan_tag(&r.oid, &mut seen, &mut errors).map_err(one)?;
            }
        }
        let order = self.order(&records, errors)?;

        let mut import = Command::new("git")
            .arg("-C")
            .arg(self.top)
            .args(["-c", "core.ignorecase=false", "fast-import", "--quiet"])
            // get-mark の答えを標準出力で受ける。--quiet なのでほかには何も出ない
            .args(["--done", "--cat-blob-fd=1"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| one(format!("git fast-import を起動できない: {e}")))?;
        let result = self.import(&records, &order, &mut import);
        if result.is_err() {
            // 途中で止めたときは、fast-import が ref を書く前に終わらせる
            let _ = import.kill();
            let _ = import.wait();
            return result;
        }
        let status = import.wait().map_err(io_error)?;
        if !status.success() {
            return Err(one(format!("git fast-import が失敗した（{status}）")));
        }
        Ok(())
    }

    /// 親と、文書が指すコミットを依存として、コミットを並べる。同じ順位なら元の流れの順にする
    fn order(
        &mut self,
        records: &[CommitRecord],
        mut errors: Errors,
    ) -> Result<Vec<usize>, Errors> {
        let by_mark: HashMap<&str, usize> = records
            .iter()
            .enumerate()
            .map(|(i, r)| (r.mark.as_str(), i))
            .collect();
        let by_original: HashMap<&str, usize> = records
            .iter()
            .enumerate()
            .map(|(i, r)| (r.original.as_str(), i))
            .collect();

        let mut scanned: HashMap<String, Vec<String>> = HashMap::new();
        let mut deps: Vec<HashSet<usize>> = Vec::with_capacity(records.len());
        for record in records {
            let short = short(&record.original);
            let mut mine = HashSet::new();
            for parent in &record.parents {
                match by_mark.get(parent.as_str()) {
                    Some(i) => {
                        mine.insert(*i);
                    }
                    None => errors.push(format!(
                        "コミット {short} の親 {parent} が fast-export の流れに無い"
                    )),
                }
            }
            let mut cited = self.references(
                &format!("コミット {short} の本文"),
                &record.message,
                &mut errors,
            );
            for op in &record.ops {
                let FileOp::Modify { mode, oid, path } = op else {
                    continue;
                };
                if mode == MODE_LINK || mode == MODE_GITLINK {
                    continue;
                }
                if !scanned.contains_key(oid) {
                    let data = self.cat.object(oid).map_err(one)?;
                    let label = format!("{}（コミット {short}）", String::from_utf8_lossy(path));
                    let found = if is_binary(&data) {
                        Vec::new()
                    } else {
                        self.references(&label, &data, &mut errors)
                    };
                    scanned.insert(oid.clone(), found);
                }
                cited.extend(scanned[oid].iter().cloned());
            }
            for commit in cited {
                if commit == record.original {
                    errors.push(format!(
                        "コミット {short} の文書か本文が、そのコミット自身を指す"
                    ));
                } else if let Some(i) = by_original.get(commit.as_str()) {
                    mine.insert(*i);
                }
            }
            deps.push(mine);
        }
        if !errors.is_empty() {
            return Err(errors);
        }

        let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); records.len()];
        let mut waiting: Vec<usize> = vec![0; records.len()];
        for (i, mine) in deps.iter().enumerate() {
            waiting[i] = mine.len();
            for d in mine {
                dependents[*d].push(i);
            }
        }
        let mut ready: BinaryHeap<Reverse<usize>> = (0..records.len())
            .filter(|i| waiting[*i] == 0)
            .map(Reverse)
            .collect();
        let mut order = Vec::with_capacity(records.len());
        while let Some(Reverse(i)) = ready.pop() {
            order.push(i);
            for d in &dependents[i] {
                waiting[*d] -= 1;
                if waiting[*d] == 0 {
                    ready.push(Reverse(*d));
                }
            }
        }
        if order.len() < records.len() {
            let stuck: Vec<&str> = (0..records.len())
                .filter(|i| waiting[*i] > 0)
                .map(|i| short(&records[i].original))
                .collect();
            return Err(one(format!(
                "文書が指すコミットが循環している: {}",
                stuck.join(" ")
            )));
        }
        Ok(order)
    }

    fn import(
        &mut self,
        records: &[CommitRecord],
        order: &[usize],
        import: &mut Child,
    ) -> Result<(), Errors> {
        let mut out = import.stdin.take().expect("piped stdin");
        let mut answers = BufReader::new(import.stdout.take().expect("piped stdout"));
        // ブロブの印は、コミットの印の後から振る
        let mut next_mark = records
            .iter()
            .filter_map(|r| r.mark.trim_start_matches(':').parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        let mut blob_marks: HashMap<(String, bool), String> = HashMap::new();

        for &i in order {
            let record = &records[i];
            let short = short(&record.original);
            let mut errors = Vec::new();
            let mut emit = Vec::new();
            let mut ops = Vec::new();
            for op in &record.ops {
                match op {
                    FileOp::Modify { mode, oid, path } if mode != MODE_GITLINK => {
                        let key = (oid.clone(), mode == MODE_LINK);
                        let mark = match blob_marks.get(&key) {
                            Some(mark) => mark.clone(),
                            None => {
                                let data = self.cat.object(oid).map_err(one)?;
                                let label = format!(
                                    "{}（コミット {short}）",
                                    String::from_utf8_lossy(path)
                                );
                                let (data, count) =
                                    self.transform_blob(&label, mode, &data, &mut errors);
                                self.refs_replaced += count;
                                let mark = format!(":{next_mark}");
                                next_mark += 1;
                                emit.extend_from_slice(
                                    format!("blob\nmark {mark}\ndata {}\n", data.len()).as_bytes(),
                                );
                                emit.extend_from_slice(&data);
                                emit.push(b'\n');
                                blob_marks.insert(key, mark.clone());
                                mark
                            }
                        };
                        ops.extend_from_slice(format!("M {mode} {mark} ").as_bytes());
                        ops.extend_from_slice(path);
                        ops.push(b'\n');
                    }
                    FileOp::Modify { mode, oid, path } => {
                        ops.extend_from_slice(format!("M {mode} {oid} ").as_bytes());
                        ops.extend_from_slice(path);
                        ops.push(b'\n');
                    }
                    FileOp::Other(line) => {
                        ops.extend_from_slice(line);
                        ops.push(b'\n');
                    }
                }
            }
            let (message, count) = self.transform(
                &format!("コミット {short} の本文"),
                &record.message,
                &mut errors,
            );
            self.refs_replaced += count;
            if !errors.is_empty() {
                return Err(errors);
            }

            // 親の無いコミットの前でブランチを空にする（fast-import は from の無いコミットを今の先に積む）
            if record.parents.is_empty() {
                emit.extend_from_slice(format!("reset {SCRATCH}\n\n").as_bytes());
            }
            emit.extend_from_slice(format!("commit {SCRATCH}\nmark {}\n", record.mark).as_bytes());
            emit.extend_from_slice(&record.header);
            emit.extend_from_slice(format!("data {}\n", message.len()).as_bytes());
            emit.extend_from_slice(&message);
            emit.push(b'\n');
            for (n, parent) in record.parents.iter().enumerate() {
                let key = if n == 0 { "from" } else { "merge" };
                emit.extend_from_slice(format!("{key} {parent}\n").as_bytes());
            }
            emit.extend_from_slice(&ops);
            emit.extend_from_slice(format!("\nget-mark {}\n", record.mark).as_bytes());
            out.write_all(&emit)
                .and_then(|()| out.flush())
                .map_err(io_error)?;

            let mut answer = String::new();
            answers.read_line(&mut answer).map_err(io_error)?;
            let last = answer.trim().to_string();
            if !is_full_hash(&last) {
                return Err(one(format!(
                    "fast-import の get-mark の答えが番号でない: {answer:?}"
                )));
            }
            self.finals.insert(record.original.clone(), last);
        }
        out.write_all(b"done\n").map_err(io_error)?;
        Ok(())
    }

    /// タグ `oid`（タグを指すタグは、指す方も）の本文の止める理由を集める
    fn scan_tag(
        &mut self,
        oid: &str,
        seen: &mut HashSet<String>,
        errors: &mut Errors,
    ) -> Result<(), String> {
        if !seen.insert(oid.to_string()) {
            return Ok(());
        }
        let data = self.cat.object(oid)?;
        let (headers, message) = split_object(&data);
        let value = |key: &[u8]| text(&values(&headers, key).concat());
        let name = value(b"tag");
        self.references(
            &format!("タグ {name} の本文"),
            strip_signature(message),
            errors,
        );
        if value(b"type") == "tag" {
            self.scan_tag(&value(b"object"), seen, errors)?;
        }
        Ok(())
    }

    /// 注釈つきのタグを、指すものを書き換えた後のものにして作り直す
    fn tags(&mut self, refs: &[Ref]) -> Result<(), Errors> {
        for r in refs {
            if r.kind == "tag" {
                self.make_tag(&r.oid)?;
            }
        }
        Ok(())
    }

    /// タグのオブジェクト `oid` を作り直し、新しい番号を返す（タグを指すタグは、指す方を先に作る）
    fn make_tag(&mut self, oid: &str) -> Result<String, Errors> {
        if let Some(done) = self.tag_finals.get(oid) {
            return Ok(done.clone());
        }
        let data = self.cat.object(oid).map_err(one)?;
        let (headers, message) = split_object(&data);
        let value = |key: &[u8]| text(&values(&headers, key).concat());
        let (object, kind, name) = (value(b"object"), value(b"type"), value(b"tag"));
        // 書き換えるタグは、たどるとコミットに着くもの（split_refs）なので、指すのはコミットかタグ
        let object = match kind.as_str() {
            "commit" => self.finals.get(&object).cloned().ok_or_else(|| {
                one(format!(
                    "タグ {name} が指すコミット {object} を書き換えていない"
                ))
            })?,
            "tag" => self.make_tag(&object)?,
            other => {
                return Err(one(format!(
                    "タグ {name} が指す {object} が、コミットでもタグでもない（{other}）"
                )));
            }
        };
        let mut errors = Vec::new();
        let (message, count) = self.transform(
            &format!("タグ {name} の本文"),
            strip_signature(message),
            &mut errors,
        );
        if !errors.is_empty() {
            return Err(errors);
        }
        self.refs_replaced += count;
        let mut body = format!("object {object}\ntype {kind}\ntag {name}\n").into_bytes();
        for tagger in values(&headers, b"tagger") {
            body.extend_from_slice(b"tagger ");
            body.extend_from_slice(&tagger);
            body.push(b'\n');
        }
        body.push(b'\n');
        body.extend_from_slice(&message);
        let new = git::git_stdin(self.top, &["mktag"], &body).map_err(one)?;
        let new = String::from_utf8_lossy(&new).trim().to_string();
        self.tag_finals.insert(oid.to_string(), new.clone());
        Ok(new)
    }

    /// 元の全てのコミットとタグについて、書き換えた後のものが、番号の置き換えだけを当てたものかを確かめる
    fn verify(&mut self, commits: &[String]) -> Result<(), Errors> {
        let mut errors = Vec::new();
        let mut seen_trees = HashSet::new();
        let mut seen_blobs = HashSet::new();
        for commit in commits {
            let Some(last) = self.finals.get(commit).cloned() else {
                errors.push(format!("コミット {commit} を書き換えていない"));
                continue;
            };
            let old = self.cat.object(commit).map_err(one)?;
            let new = self.cat.object(&last).map_err(one)?;
            let (old_headers, old_message) = split_object(&old);
            let (new_headers, new_message) = split_object(&new);
            let short = short(commit);
            for key in [&b"author"[..], b"committer", b"encoding"] {
                if values(&old_headers, key) != values(&new_headers, key) {
                    errors.push(format!(
                        "検証: コミット {short} の {} が変わった",
                        String::from_utf8_lossy(key)
                    ));
                }
            }
            let parents: Vec<Vec<u8>> = values(&old_headers, b"parent")
                .iter()
                .map(|p| {
                    let p = text(p);
                    self.finals.get(&p).cloned().unwrap_or(p).into_bytes()
                })
                .collect();
            if parents != values(&new_headers, b"parent") {
                errors.push(format!("検証: コミット {short} の親が合わない"));
            }
            let mut ignored = Vec::new();
            let (expected, _) = self.transform(
                &format!("コミット {short} の本文"),
                old_message,
                &mut ignored,
            );
            if expected != new_message {
                errors.push(format!("検証: コミット {short} の本文が合わない"));
            }
            let old_tree = text(&values(&old_headers, b"tree").concat());
            let new_tree = text(&values(&new_headers, b"tree").concat());
            self.verify_tree(
                &old_tree,
                &new_tree,
                "",
                short,
                &mut seen_trees,
                &mut seen_blobs,
                &mut errors,
            )
            .map_err(one)?;
        }
        let tags: Vec<(String, String)> = self
            .tag_finals
            .iter()
            .map(|(o, n)| (o.clone(), n.clone()))
            .collect();
        for (old_oid, new_oid) in tags {
            let old = self.cat.object(&old_oid).map_err(one)?;
            let new = self.cat.object(&new_oid).map_err(one)?;
            let (old_headers, old_message) = split_object(&old);
            let (new_headers, new_message) = split_object(&new);
            let name = text(&values(&old_headers, b"tag").concat());
            let old_object = text(&values(&old_headers, b"object").concat());
            let expected_object = self
                .finals
                .get(&old_object)
                .or_else(|| self.tag_finals.get(&old_object));
            if expected_object.map(String::as_bytes)
                != Some(values(&new_headers, b"object").concat().as_slice())
            {
                errors.push(format!("検証: タグ {name} が指すものが合わない"));
            }
            for key in [&b"type"[..], b"tag", b"tagger"] {
                if values(&old_headers, key) != values(&new_headers, key) {
                    errors.push(format!(
                        "検証: タグ {name} の {} が変わった",
                        String::from_utf8_lossy(key)
                    ));
                }
            }
            let mut ignored = Vec::new();
            let (expected, _) = self.transform(
                &format!("タグ {name} の本文"),
                strip_signature(old_message),
                &mut ignored,
            );
            if expected != new_message {
                errors.push(format!("検証: タグ {name} の本文が合わない"));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn verify_tree(
        &mut self,
        old: &str,
        new: &str,
        prefix: &str,
        commit: &str,
        seen_trees: &mut HashSet<(String, String)>,
        seen_blobs: &mut HashSet<(String, String, String)>,
        errors: &mut Errors,
    ) -> Result<(), String> {
        if !seen_trees.insert((old.to_string(), new.to_string())) {
            return Ok(());
        }
        let old_entries = parse_tree(&self.cat.object(old)?, self.hash_bytes);
        let new_entries = parse_tree(&self.cat.object(new)?, self.hash_bytes);
        if old_entries.len() != new_entries.len() {
            errors.push(format!(
                "検証: コミット {commit} の {prefix} のファイルの数が合わない"
            ));
            return Ok(());
        }
        for (o, n) in old_entries.iter().zip(&new_entries) {
            let path = format!("{prefix}{}", String::from_utf8_lossy(&o.name));
            if o.name != n.name || o.mode != n.mode {
                errors.push(format!(
                    "検証: コミット {commit} の {path} の名前か種類が合わない"
                ));
                continue;
            }
            match o.mode.as_str() {
                "40000" => self.verify_tree(
                    &o.oid,
                    &n.oid,
                    &format!("{path}/"),
                    commit,
                    seen_trees,
                    seen_blobs,
                    errors,
                )?,
                // サブモジュールのコミットは書き換えない
                MODE_GITLINK => {
                    if o.oid != n.oid {
                        errors.push(format!("検証: コミット {commit} の {path} が変わった"));
                    }
                }
                mode => {
                    if !seen_blobs.insert((o.oid.clone(), n.oid.clone(), mode.to_string())) {
                        continue;
                    }
                    let old_data = self.cat.object(&o.oid)?;
                    let new_data = self.cat.object(&n.oid)?;
                    let mut ignored = Vec::new();
                    let (expected, _) = self.transform_blob(&path, mode, &old_data, &mut ignored);
                    if expected != new_data {
                        errors.push(format!("検証: コミット {commit} の {path} が合わない"));
                    }
                }
            }
        }
        Ok(())
    }
}

/// 書き換える ref から届くコミットを、fast-export の流れから読む（タグとブランチの命令は読み飛ばし、ref は
/// トランザクションで移す）
fn read_commits(top: &Path, refs: &[Ref]) -> Result<Vec<CommitRecord>, Errors> {
    let mut export = Command::new("git")
        .arg("-C")
        .arg(top)
        .args(FAST_EXPORT)
        .arg("--stdin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| one(format!("git fast-export を起動できない: {e}")))?;
    // fast-export は版の指定を読み終えてから流しはじめる
    export
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(&revisions(refs))
        .map_err(io_error)?;
    let mut input = Lines::new(BufReader::new(export.stdout.take().expect("piped stdout")));
    let result = parse_stream(&mut input);
    if result.is_err() {
        let _ = export.kill();
    }
    let status = export.wait().map_err(io_error)?;
    let records = result?;
    if !status.success() {
        return Err(one(format!("git fast-export が失敗した（{status}）")));
    }
    Ok(records)
}

fn parse_stream(input: &mut Lines<impl BufRead>) -> Result<Vec<CommitRecord>, Errors> {
    let mut records = Vec::new();
    loop {
        let Some(line) = input.next().map_err(io_error)? else {
            return Err(one("fast-export の流れが done の前に終わった".to_string()));
        };
        if line.is_empty() || line == b"feature done" {
            continue;
        }
        let (command, _) = split_command(&line);
        match command {
            b"reset" => {
                if input
                    .peek()
                    .map_err(io_error)?
                    .is_some_and(|l| l.starts_with(b"from "))
                {
                    input.next().map_err(io_error)?;
                }
            }
            b"commit" => records.push(read_commit(input)?),
            b"tag" => skip_tag(input)?,
            b"done" => return Ok(records),
            _ => return Err(one(unexpected("流れ", &line))),
        }
    }
}

fn read_commit(input: &mut Lines<impl BufRead>) -> Result<CommitRecord, Errors> {
    let mut record = CommitRecord {
        mark: String::new(),
        original: String::new(),
        header: Vec::new(),
        message: Vec::new(),
        parents: Vec::new(),
        ops: Vec::new(),
    };
    loop {
        let line = input.next().map_err(io_error)?.unwrap_or_default();
        let (key, value) = split_command(&line);
        match key {
            b"mark" => record.mark = text(value),
            b"original-oid" => record.original = text(value),
            b"author" | b"committer" | b"encoding" => {
                record.header.extend_from_slice(&line);
                record.header.push(b'\n');
            }
            b"data" => {
                record.message = input.data(value).map_err(io_error)?;
                break;
            }
            _ => return Err(one(unexpected("commit", &line))),
        }
    }
    while let Some(line) = input.peek().map_err(io_error)? {
        if line.is_empty() {
            input.next().map_err(io_error)?;
            break;
        }
        let (key, _) = split_command(line);
        if !matches!(
            key,
            b"from" | b"merge" | b"M" | b"D" | b"C" | b"R" | b"deleteall"
        ) {
            break;
        }
        let line = input.next().map_err(io_error)?.unwrap_or_default();
        let (key, value) = split_command(&line);
        match key {
            b"from" | b"merge" => {
                let parent = text(value);
                if !parent.starts_with(':') {
                    return Err(one(format!(
                        "コミット {} の親 {parent} が fast-export の流れの外にある",
                        short(&record.original)
                    )));
                }
                record.parents.push(parent);
            }
            b"M" => {
                // M <mode> <ブロブの番号> <パス>（パスは引用されていてもそのまま渡す）
                let mut parts = value.splitn(3, |b| *b == b' ');
                match (parts.next(), parts.next(), parts.next()) {
                    (Some(mode), Some(oid), Some(path)) => record.ops.push(FileOp::Modify {
                        mode: text(mode),
                        oid: text(oid),
                        path: path.to_vec(),
                    }),
                    _ => return Err(one(unexpected("commit", &line))),
                }
            }
            _ => record.ops.push(FileOp::Other(line)),
        }
    }
    Ok(record)
}

/// 注釈つきのタグの命令を読み飛ばす（タグはオブジェクトから作り直す）
fn skip_tag(input: &mut Lines<impl BufRead>) -> Result<(), Errors> {
    loop {
        let line = input.next().map_err(io_error)?.unwrap_or_default();
        let (key, value) = split_command(&line);
        match key {
            b"mark" | b"from" | b"original-oid" | b"tagger" => {}
            b"data" => {
                input.data(value).map_err(io_error)?;
                return Ok(());
            }
            _ => return Err(one(unexpected("tag", &line))),
        }
    }
}

/// ブランチとタグを、書き換えた後のものに 1 つのトランザクションで移し、元の ref を残す
fn move_refs(top: &Path, refs: &[Ref], rewrite: &Rewrite, head: &Head) -> Result<(), Errors> {
    let mut errors = Vec::new();
    let mut commands = String::new();
    for r in refs {
        let new = match r.kind.as_str() {
            "tag" => rewrite.tag_finals.get(&r.oid),
            _ => rewrite.finals.get(&r.oid),
        };
        let Some(new) = new else {
            errors.push(format!("{} が指す {} を書き換えていない", r.name, r.oid));
            continue;
        };
        let rest = r.name.strip_prefix("refs/").unwrap_or(&r.name);
        commands.push_str(&format!("update {} {new} {}\n", r.name, r.oid));
        commands.push_str(&format!("create {ORIGINAL_NS}{rest} {}\n", r.oid));
    }
    if let Head::Detached(old) = head {
        match rewrite.finals.get(old) {
            Some(new) => commands.push_str(&format!("option no-deref\nupdate HEAD {new} {old}\n")),
            None => errors.push(format!("HEAD が指す {old} を書き換えていない")),
        }
    }
    commands.push_str(&format!("delete {SCRATCH}\n"));
    if !errors.is_empty() {
        return Err(errors);
    }
    git::git_stdin(top, &["update-ref", "--stdin"], commands.as_bytes())
        .map(|_| ())
        .map_err(one)
}

/// fast-import がコミットを積んだブランチを消す（無ければ何もしない）
fn delete_scratch(top: &Path) -> Result<(), String> {
    let exists = git::run_git(top, &["rev-parse", "--verify", "-q", SCRATCH])?;
    if exists.status.success() {
        git::git(top, &["update-ref", "-d", SCRATCH])?;
    }
    Ok(())
}

/// 元の番号から最後の番号への対応表を、filter-repo の commit-map と同じ形で書く
fn write_map(path: &Path, map: &Map, finals: &HashMap<String, String>) -> Result<(), String> {
    let mut text = String::from("old new\n");
    for (old, target) in map.targets() {
        let last = match target {
            Target::Pruned => "0".repeat(old.len()),
            Target::Changed(new) => finals.get(new).cloned().unwrap_or_else(|| new.clone()),
            Target::Unchanged => finals.get(old).cloned().unwrap_or_else(|| old.to_string()),
        };
        text.push_str(&format!("{old} {last}\n"));
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// 1 行ずつ読み、次の行を覗ける読み手。`data` の中身はバイト数で読む
struct Lines<R> {
    reader: R,
    peeked: Option<Vec<u8>>,
}

impl<R: BufRead> Lines<R> {
    fn new(reader: R) -> Self {
        Lines {
            reader,
            peeked: None,
        }
    }

    fn read(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        let mut line = Vec::new();
        if self.reader.read_until(b'\n', &mut line)? == 0 {
            return Ok(None);
        }
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        Ok(Some(line))
    }

    fn next(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        match self.peeked.take() {
            Some(line) => Ok(Some(line)),
            None => self.read(),
        }
    }

    fn peek(&mut self) -> std::io::Result<Option<&Vec<u8>>> {
        if self.peeked.is_none() {
            self.peeked = self.read()?;
        }
        Ok(self.peeked.as_ref())
    }

    /// `data <n>` の後の n バイトを読む。後に続く省いてよい改行も読み飛ばす
    fn data(&mut self, count: &[u8]) -> std::io::Result<Vec<u8>> {
        let count: usize = text(count).parse().map_err(|_| {
            std::io::Error::other(format!(
                "data の長さが読めない: {}",
                String::from_utf8_lossy(count)
            ))
        })?;
        let mut data = vec![0; count];
        self.reader.read_exact(&mut data)?;
        if self.reader.fill_buf()?.first() == Some(&b'\n') {
            self.reader.consume(1);
        }
        Ok(data)
    }
}

/// `git cat-file --batch` で、オブジェクトを 1 つずつ読む
struct CatFile {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}

impl CatFile {
    fn new(top: &Path) -> Result<Self, String> {
        let mut child = Command::new("git")
            .arg("-C")
            .arg(top)
            .args(["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("git cat-file を起動できない: {e}"))?;
        let input = child.stdin.take();
        let output = BufReader::new(child.stdout.take().expect("piped stdout"));
        Ok(CatFile {
            child,
            input,
            output,
        })
    }

    fn object(&mut self, oid: &str) -> Result<Vec<u8>, String> {
        let io = |e: std::io::Error| format!("git cat-file: {e}");
        let input = self.input.as_mut().expect("open until dropped");
        writeln!(input, "{oid}").map_err(io)?;
        input.flush().map_err(io)?;
        let mut header = String::new();
        self.output.read_line(&mut header).map_err(io)?;
        let size: usize = header
            .trim_end()
            .rsplit(' ')
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| format!("git cat-file が {oid} を読めない: {}", header.trim_end()))?;
        let mut data = vec![0; size + 1];
        self.output.read_exact(&mut data).map_err(io)?;
        data.pop();
        Ok(data)
    }
}

impl Drop for CatFile {
    fn drop(&mut self) {
        // 入力を閉じると cat-file は終わる。待って片付ける
        drop(self.input.take());
        let _ = self.child.wait();
    }
}

/// コミットかタグのオブジェクトの見出しの行（キーと値）
type Header = (Vec<u8>, Vec<u8>);

/// コミットかタグのオブジェクトを、見出しの行と本文に分ける
fn split_object(data: &[u8]) -> (Vec<Header>, &[u8]) {
    let mut headers: Vec<Header> = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let end = data[i..]
            .iter()
            .position(|b| *b == b'\n')
            .map_or(data.len(), |p| i + p);
        let line = &data[i..end];
        i = end + 1;
        if line.is_empty() {
            return (headers, data.get(i..).unwrap_or_default());
        }
        // 先頭が空白の行は、前の見出しの続き（gpgsig など）
        if line.first() == Some(&b' ') {
            if let Some((_, value)) = headers.last_mut() {
                value.push(b'\n');
                value.extend_from_slice(line);
            }
            continue;
        }
        let (key, value) = split_command(line);
        headers.push((key.to_vec(), value.to_vec()));
    }
    (headers, &[])
}

fn values(headers: &[Header], key: &[u8]) -> Vec<Vec<u8>> {
    headers
        .iter()
        .filter(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .collect()
}

/// タグの本文の末尾の署名を外す。git の parse_signed_buffer と同じく、署名の始まりの印で始まる行のうち最後の
/// 行から後を署名とする。中身を変えると署名は合わないので、fast-export の --signed-tags=strip と同じく外す
fn strip_signature(message: &[u8]) -> &[u8] {
    // git の gpg-interface.c の sigs[] にある印
    const MARKERS: [&[u8]; 4] = [
        b"-----BEGIN PGP SIGNATURE-----",
        b"-----BEGIN PGP MESSAGE-----",
        b"-----BEGIN SIGNED MESSAGE-----",
        b"-----BEGIN SSH SIGNATURE-----",
    ];
    let mut last = None;
    let mut start = 0;
    while start < message.len() {
        let line = &message[start..];
        if MARKERS.iter().any(|m| line.starts_with(m)) {
            last = Some(start);
        }
        match line.iter().position(|b| *b == b'\n') {
            Some(p) => start += p + 1,
            None => break,
        }
    }
    match last {
        Some(at) => &message[..at],
        None => message,
    }
}

struct TreeEntry {
    mode: String,
    name: Vec<u8>,
    oid: String,
}

/// ツリーのオブジェクトの中身（`<mode> <name>\0<番号のバイト列>` の並び）を読む。`width` は番号のバイト数
fn parse_tree(data: &[u8], width: usize) -> Vec<TreeEntry> {
    let mut entries = Vec::new();
    let mut i = 0;
    while i < data.len() {
        let Some(space) = data[i..].iter().position(|b| *b == b' ') else {
            break;
        };
        let mode = text(&data[i..i + space]);
        let name_start = i + space + 1;
        let Some(nul) = data[name_start..].iter().position(|b| *b == 0) else {
            break;
        };
        let name = data[name_start..name_start + nul].to_vec();
        let oid_start = name_start + nul + 1;
        let oid: String = data[oid_start..oid_start + width]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        entries.push(TreeEntry { mode, name, oid });
        i = oid_start + width;
    }
    entries
}

/// リポジトリのオブジェクトの番号のバイト数
fn hash_bytes(top: &Path) -> Result<usize, String> {
    let format = git::git(top, &["rev-parse", "--show-object-format"])?;
    match String::from_utf8_lossy(&format).trim() {
        "sha1" => Ok(20),
        "sha256" => Ok(32),
        other => Err(format!("知らないオブジェクトの形式: {other}")),
    }
}

fn is_binary(data: &[u8]) -> bool {
    data[..data.len().min(FIRST_FEW_BYTES)].contains(&0)
}

fn short(oid: &str) -> &str {
    &oid[..oid.len().min(12)]
}

/// 行を、最初の空白の前（命令やキー）と後に分ける
fn split_command(line: &[u8]) -> (&[u8], &[u8]) {
    match line.iter().position(|b| *b == b' ') {
        Some(p) => (&line[..p], &line[p + 1..]),
        None => (line, &[]),
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn unexpected(context: &str, line: &[u8]) -> String {
    format!(
        "fast-export の {context} で知らない行: {}",
        String::from_utf8_lossy(line)
    )
}
