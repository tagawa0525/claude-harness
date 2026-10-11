//! rewrite-history: 履歴の中の文書と本文の番号を、書き換えた後の最後の番号に付け直す（ADR-0013）。
//!
//! 全てのブランチとタグを `git fast-export` で親から順に読み、テキストのブロブとコミットとタグの本文の番号を
//! 置き換えて `git fast-import` に渡す。コミットを 1 つ渡すたびに `get-mark` で最後の番号を受け取り、後のコミットの
//! 置き換えに使う。書き換えた履歴は `refs/commit-refs/new/` に作り、元の履歴と突き合わせてから、ブランチとタグを
//! 1 つのトランザクションで移す。元の ref は `refs/commit-refs/original/` に残す。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use commit_refs::{Map, Target};

use crate::{git, read_map};

const NEW_NS: &str = "refs/commit-refs/new/";
const ORIGINAL_NS: &str = "refs/commit-refs/original/";

/// テキストかどうかを見る先頭のバイト数（git の xdiff-interface.c の FIRST_FEW_BYTES と同じ）
const FIRST_FEW_BYTES: usize = 8000;

/// filter-repo 2.47.0 の fep_cmd と同じ設定に、署名つきのコミットの扱いと、本文を元の符号化のまま渡す設定を足す
/// （番号を置き換えると中身が変わり、元の署名は合わない）
const FAST_EXPORT: &[&str] = &[
    "fast-export",
    "--show-original-ids",
    "--signed-tags=strip",
    "--signed-commits=strip",
    "--tag-of-filtered-object=rewrite",
    "--fake-missing-tagger",
    "--reference-excluded-parents",
    "--use-done-feature",
    "--mark-tags",
    "--reencode=no",
];

pub fn run(map_path: &Path) -> Result<(), Vec<String>> {
    let text = read_map(Some(map_path))?;
    let mut map = Map::parse(&text)?;
    let top = git::toplevel().map_err(|e| vec![e])?;
    let map_file = map_file(&top).map_err(|e| vec![e])?;
    preflight(&top, &map_file).map_err(|e| vec![e])?;

    let refs = list_refs(&top).map_err(|e| vec![e])?;
    if refs.is_empty() {
        return Err(vec!["書き換えるブランチとタグが無い".to_string()]);
    }
    let commits = rev_list(&top, &refs).map_err(|e| vec![e])?;
    map.add_unchanged(commits.iter().map(String::as_str));
    let current: HashSet<String> = commits.iter().cloned().collect();
    let head = head_state(&top).map_err(|e| vec![e])?;

    let mut rewrite = Rewrite {
        top: &top,
        map: &map,
        current: &current,
        finals: HashMap::new(),
        tag_finals: HashMap::new(),
        refs_replaced: 0,
        hash_bytes: hash_bytes(&top).map_err(|e| vec![e])?,
    };
    let result = rewrite
        .stream()
        .and_then(|tags| rewrite.verify(&commits, &tags));
    if let Err(mut errors) = result {
        if let Err(e) = delete_new_refs(&top) {
            errors.push(e);
        }
        errors.push("ブランチとタグは動かしていない".to_string());
        return Err(errors);
    }

    if let Err(mut errors) = move_refs(&top, &refs, &rewrite, &head) {
        if let Err(e) = delete_new_refs(&top) {
            errors.push(e);
        }
        errors.push("ブランチとタグは動かしていない".to_string());
        return Err(errors);
    }
    // 始める前に作業ツリーに変更が無いことを確かめたので、新しい HEAD に合わせても失うものは無い
    git::git(&top, &["reset", "-q", "--hard"]).map_err(|e| {
        vec![
            e,
            "ブランチとタグは移した。作業ツリーは git reset --hard で合わせる".to_string(),
        ]
    })?;
    write_map(&map_file, &map, &rewrite.finals).map_err(|e| vec![e])?;

    let changed = rewrite.finals.iter().filter(|(c, f)| c != f).count();
    let shown = map_file.strip_prefix(&top).unwrap_or(&map_file);
    print!(
        "COMMITS: {}\nCHANGED: {changed}\nREFS: {}\nMAP: {}\n",
        rewrite.finals.len(),
        rewrite.refs_replaced,
        shown.display()
    );
    Ok(())
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

/// 作業ツリーに変更が無く、前の書き換えの ref と対応表が残っていないこと
fn preflight(top: &Path, map_file: &Path) -> Result<(), String> {
    let status = git::git(top, &["status", "--porcelain", "--untracked-files=no"])?;
    if !status.is_empty() {
        return Err(format!(
            "作業ツリーに変更がある（コミットするか片付けてから）:\n{}",
            String::from_utf8_lossy(&status).trim_end()
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

/// 書き換えるブランチとタグの名前と、指すオブジェクト
fn list_refs(top: &Path) -> Result<Vec<(String, String)>, String> {
    let out = git::git(
        top,
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/heads/",
            "refs/tags/",
        ],
    )?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .filter_map(|line| line.split_once(' '))
        .map(|(name, oid)| (name.to_string(), oid.to_string()))
        .collect())
}

/// ブランチとタグから届く全てのコミット
fn rev_list(top: &Path, refs: &[(String, String)]) -> Result<Vec<String>, String> {
    let mut args = vec!["rev-list"];
    args.extend(refs.iter().map(|(name, _)| name.as_str()));
    let out = git::git(top, &args)?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .map(str::to_string)
        .collect())
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

/// 注釈つきのタグ。fast-import の `tag` は refs/tags/ に ref を作るので、流れからは外し、コミットを入れ終えてから
/// `git mktag` で作る
struct TagSpec {
    name: String,
    mark: Option<String>,
    from: String,
    original: String,
    tagger: Vec<u8>,
    message: Vec<u8>,
}

struct Rewrite<'a> {
    top: &'a Path,
    map: &'a Map,
    current: &'a HashSet<String>,
    /// 今のコミット → 書き換えた後の最後のコミット
    finals: HashMap<String, String>,
    /// 今の注釈つきのタグ → 書き換えた後のタグ
    tag_finals: HashMap<String, String>,
    refs_replaced: usize,
    /// オブジェクトの番号のバイト数（SHA-1 は 20、SHA-256 は 32）
    hash_bytes: usize,
}

impl Rewrite<'_> {
    /// 文書の語が当たった旧を、書き換えた後の最後の番号にする。`finals` に無い今のコミットは、まだ流していない
    fn resolve(
        &self,
        label: &str,
        token: &str,
        old: &str,
        target: &Target,
        errors: &mut Vec<String>,
    ) -> Option<String> {
        let commit = match target {
            Target::Pruned => {
                errors.push(format!(
                    "{label}: {token} は書き換えで消えたコミット {old} を指す"
                ));
                return None;
            }
            Target::Changed(new) => new.as_str(),
            Target::Unchanged => old,
        };
        match self.finals.get(commit) {
            Some(last) => Some(last.clone()),
            None if self.current.contains(commit) => {
                errors.push(format!(
                    "{label}: {token} が指すコミット {commit} を、まだ書き換えていない（祖先でないコミットを指す）"
                ));
                None
            }
            // 書き換えるブランチとタグから届かないコミットは、この書き換えで変わらない
            None => Some(commit.to_string()),
        }
    }

    /// テキストのブロブか本文の番号を置き換えた内容と、置き換えた語の数
    fn transform(&self, label: &str, data: &[u8], errors: &mut Vec<String>) -> (Vec<u8>, usize) {
        if is_binary(data) {
            return (data.to_vec(), 0);
        }
        self.map
            .replace_by(label, data, errors, |token, old, target, errors| {
                self.resolve(label, token, old, target, errors)
            })
    }

    /// fast-export の流れを書き換えて fast-import に渡し、注釈つきのタグを作る。作ったタグの元の情報を返す
    fn stream(&mut self) -> Result<Vec<TagSpec>, Vec<String>> {
        let mut export = Command::new("git")
            .arg("-C")
            .arg(self.top)
            .args(FAST_EXPORT)
            .args(["--branches", "--tags"])
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| vec![format!("git fast-export を起動できない: {e}")])?;
        let mut import = Command::new("git")
            .arg("-C")
            .arg(self.top)
            .args(["-c", "core.ignorecase=false", "fast-import", "--quiet"])
            // get-mark の答えを標準出力で受ける。--quiet なのでほかには何も出ない
            .args(["--done", "--cat-blob-fd=1"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| vec![format!("git fast-import を起動できない: {e}")])?;

        let result = self.pump(&mut export, &mut import);
        if let Err(errors) = result {
            // 途中で止めたときは、fast-import が ref を書く前に終わらせる
            let _ = import.kill();
            let _ = export.kill();
            let _ = import.wait();
            let _ = export.wait();
            return Err(errors);
        }
        let status = import
            .wait()
            .map_err(|e| vec![format!("git fast-import: {e}")])?;
        if !status.success() {
            return Err(vec![format!("git fast-import が失敗した（{status}）")]);
        }
        let status = export
            .wait()
            .map_err(|e| vec![format!("git fast-export: {e}")])?;
        if !status.success() {
            return Err(vec![format!("git fast-export が失敗した（{status}）")]);
        }
        let tags = result?;
        self.make_tags(&tags)?;
        Ok(tags)
    }

    fn pump(
        &mut self,
        export: &mut Child,
        import: &mut Child,
    ) -> Result<Vec<TagSpec>, Vec<String>> {
        let mut input = Lines::new(BufReader::new(export.stdout.take().expect("piped stdout")));
        let mut out = import.stdin.take().expect("piped stdin");
        let mut answers = BufReader::new(import.stdout.take().expect("piped stdout"));
        let io = |e: std::io::Error| {
            vec![format!(
                "fast-export と fast-import のやりとりに失敗した: {e}"
            )]
        };

        let mut blobs: HashMap<String, (String, Vec<u8>)> = HashMap::new();
        let mut marks: HashMap<String, String> = HashMap::new();
        let mut tags = Vec::new();
        loop {
            let Some(line) = input.next().map_err(io)? else {
                return Err(vec!["fast-export の流れが done の前に終わった".to_string()]);
            };
            if line.is_empty() || line == b"feature done" {
                continue;
            }
            let (command, rest) = split_command(&line);
            match command {
                b"blob" => {
                    let mut mark = String::new();
                    let mut original = String::new();
                    let data = loop {
                        let line = input.next().map_err(io)?.unwrap_or_default();
                        let (key, value) = split_command(&line);
                        match key {
                            b"mark" => mark = text(value),
                            b"original-oid" => original = text(value),
                            b"data" => break input.data(value).map_err(io)?,
                            _ => return Err(vec![unexpected("blob", &line)]),
                        }
                    };
                    blobs.insert(mark, (original, data));
                }
                b"reset" => {
                    let mut emit = format!("reset {}\n", new_ref(&text(rest))).into_bytes();
                    if input
                        .peek()
                        .map_err(io)?
                        .is_some_and(|l| l.starts_with(b"from "))
                    {
                        let from = input.next().map_err(io)?.unwrap_or_default();
                        emit.extend_from_slice(&from);
                        emit.push(b'\n');
                    }
                    emit.push(b'\n');
                    out.write_all(&emit).map_err(io)?;
                }
                b"commit" => {
                    self.commit(
                        &text(rest),
                        &mut input,
                        &mut out,
                        &mut answers,
                        &mut blobs,
                        &mut marks,
                    )?;
                }
                b"tag" => tags.push(read_tag(&text(rest), &mut input).map_err(|e| vec![e])?),
                b"done" => break,
                _ => return Err(vec![unexpected("流れ", &line)]),
            }
        }
        out.write_all(b"done\n").map_err(io)?;
        drop(out);
        self.resolve_tag_targets(&mut tags, &marks)?;
        Ok(tags)
    }

    #[allow(clippy::too_many_arguments)]
    fn commit(
        &mut self,
        name: &str,
        input: &mut Lines<impl BufRead>,
        out: &mut ChildStdin,
        answers: &mut BufReader<ChildStdout>,
        blobs: &mut HashMap<String, (String, Vec<u8>)>,
        marks: &mut HashMap<String, String>,
    ) -> Result<(), Vec<String>> {
        let io = |e: std::io::Error| {
            vec![format!(
                "fast-export と fast-import のやりとりに失敗した: {e}"
            )]
        };
        let mut mark = String::new();
        let mut original = String::new();
        let mut header = Vec::new();
        let message = loop {
            let line = input.next().map_err(io)?.unwrap_or_default();
            let (key, value) = split_command(&line);
            match key {
                b"mark" => mark = text(value),
                b"original-oid" => original = text(value),
                b"author" | b"committer" | b"encoding" => {
                    header.extend_from_slice(&line);
                    header.push(b'\n');
                }
                b"data" => break input.data(value).map_err(io)?,
                _ => return Err(vec![unexpected("commit", &line)]),
            }
        };
        let short = &original[..original.len().min(12)];

        let mut errors = Vec::new();
        let mut emit_blobs = Vec::new();
        let mut rest = Vec::new();
        while let Some(line) = input.peek().map_err(io)? {
            let (key, value) = split_command(line);
            if line.is_empty() {
                input.next().map_err(io)?;
                break;
            }
            match key {
                b"from" | b"merge" | b"D" | b"C" | b"R" | b"deleteall" => {}
                b"M" => {
                    // M <mode> <dataref> <path>
                    let mut parts = value.splitn(3, |b| *b == b' ');
                    let (_, dataref, path) = (parts.next(), parts.next(), parts.next());
                    if let (Some(dataref), Some(path)) = (dataref, path)
                        && let Some((_, data)) = blobs.remove(&text(dataref))
                    {
                        let label =
                            format!("{}（コミット {short}）", String::from_utf8_lossy(path));
                        let (data, count) = self.transform(&label, &data, &mut errors);
                        self.refs_replaced += count;
                        emit_blobs.push((text(dataref), data));
                    }
                }
                _ => break,
            }
            let line = input.next().map_err(io)?.unwrap_or_default();
            rest.extend_from_slice(&line);
            rest.push(b'\n');
        }
        let (message, count) =
            self.transform(&format!("コミット {short} の本文"), &message, &mut errors);
        self.refs_replaced += count;
        if !errors.is_empty() {
            return Err(errors);
        }

        let mut emit = Vec::new();
        for (blob_mark, data) in emit_blobs {
            emit.extend_from_slice(
                format!("blob\nmark {blob_mark}\ndata {}\n", data.len()).as_bytes(),
            );
            emit.extend_from_slice(&data);
            emit.push(b'\n');
        }
        emit.extend_from_slice(format!("commit {}\nmark {mark}\n", new_ref(name)).as_bytes());
        emit.extend_from_slice(&header);
        emit.extend_from_slice(format!("data {}\n", message.len()).as_bytes());
        emit.extend_from_slice(&message);
        emit.push(b'\n');
        emit.extend_from_slice(&rest);
        emit.extend_from_slice(format!("\nget-mark {mark}\n").as_bytes());
        out.write_all(&emit)
            .and_then(|()| out.flush())
            .map_err(io)?;

        let mut answer = String::new();
        answers.read_line(&mut answer).map_err(io)?;
        let last = answer.trim().to_string();
        if !is_hash(&last) {
            return Err(vec![format!(
                "fast-import の get-mark の答えが番号でない: {answer:?}"
            )]);
        }
        marks.insert(mark, last.clone());
        self.finals.insert(original, last);
        Ok(())
    }

    /// 注釈つきのタグが指すものを、書き換えた後の番号にする（`from` の印を引く）
    fn resolve_tag_targets(
        &self,
        tags: &mut [TagSpec],
        marks: &HashMap<String, String>,
    ) -> Result<(), Vec<String>> {
        // タグが指すのは、コミットの印か、先に出たタグの印（タグを指すタグ）。印は make_tags で引く
        let mut tag_marks = HashSet::new();
        for tag in tags.iter_mut() {
            if let Some(last) = marks.get(&tag.from) {
                tag.from = last.clone();
            } else if !tag_marks.contains(&tag.from) {
                return Err(vec![format!(
                    "タグ {} が指す {} が、流れに出たコミットにもタグにも無い",
                    tag.name, tag.from
                )]);
            }
            if let Some(mark) = &tag.mark {
                tag_marks.insert(mark.clone());
            }
        }
        Ok(())
    }

    /// 注釈つきのタグを `git mktag` で作り、`refs/commit-refs/new/tags/` に置く
    fn make_tags(&mut self, tags: &[TagSpec]) -> Result<(), Vec<String>> {
        let mut by_mark: HashMap<String, String> = HashMap::new();
        for tag in tags {
            let (object, kind) = match by_mark.get(&tag.from) {
                Some(oid) => (oid.clone(), "tag"),
                None => (tag.from.clone(), "commit"),
            };
            let mut errors = Vec::new();
            let (message, count) = self.transform(
                &format!("タグ {} の本文", tag.name),
                &tag.message,
                &mut errors,
            );
            if !errors.is_empty() {
                return Err(errors);
            }
            self.refs_replaced += count;
            let mut body = format!("object {object}\ntype {kind}\ntag {}\n", tag.name).into_bytes();
            body.extend_from_slice(&tag.tagger);
            body.extend_from_slice(b"\n\n");
            body.extend_from_slice(&message);
            let oid = git_stdin(self.top, &["mktag"], &body).map_err(|e| vec![e])?;
            git::git(
                self.top,
                &["update-ref", &format!("{NEW_NS}tags/{}", tag.name), &oid],
            )
            .map_err(|e| vec![e])?;
            if let Some(mark) = &tag.mark {
                by_mark.insert(mark.clone(), oid.clone());
            }
            self.tag_finals.insert(tag.original.clone(), oid);
        }
        Ok(())
    }

    /// 元の全てのコミットとタグについて、書き換えた後のものが、番号の置き換えだけを当てたものかを確かめる
    fn verify(&self, commits: &[String], tags: &[TagSpec]) -> Result<(), Vec<String>> {
        let mut cat = CatFile::new(self.top).map_err(|e| vec![e])?;
        let mut errors = Vec::new();
        let mut seen_trees = HashSet::new();
        let mut seen_blobs = HashSet::new();
        for commit in commits {
            let Some(last) = self.finals.get(commit) else {
                errors.push(format!("コミット {commit} が fast-export の流れに無かった"));
                continue;
            };
            let old = cat.object(commit).map_err(|e| vec![e])?;
            let new = cat.object(last).map_err(|e| vec![e])?;
            let (old_headers, old_message) = split_object(&old);
            let (new_headers, new_message) = split_object(&new);
            let short = &commit[..12];
            let field = |headers: &[Header], key: &[u8]| -> Vec<Vec<u8>> {
                headers
                    .iter()
                    .filter(|(k, _)| k == key)
                    .map(|(_, v)| v.clone())
                    .collect()
            };
            for key in [&b"author"[..], b"committer", b"encoding"] {
                if field(&old_headers, key) != field(&new_headers, key) {
                    errors.push(format!(
                        "検証: コミット {short} の {} が変わった",
                        String::from_utf8_lossy(key)
                    ));
                }
            }
            let parents: Vec<Vec<u8>> = field(&old_headers, b"parent")
                .iter()
                .map(|p| {
                    let p = String::from_utf8_lossy(p).to_string();
                    self.finals.get(&p).cloned().unwrap_or(p).into_bytes()
                })
                .collect();
            if parents != field(&new_headers, b"parent") {
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
            let old_tree = text(&field(&old_headers, b"tree").concat());
            let new_tree = text(&field(&new_headers, b"tree").concat());
            self.verify_tree(
                &mut cat,
                &old_tree,
                &new_tree,
                "",
                short,
                &mut seen_trees,
                &mut seen_blobs,
                &mut errors,
            )
            .map_err(|e| vec![e])?;
        }
        for tag in tags {
            let Some(new_oid) = self.tag_finals.get(&tag.original) else {
                errors.push(format!("検証: タグ {} を作っていない", tag.name));
                continue;
            };
            let old = cat.object(&tag.original).map_err(|e| vec![e])?;
            let new = cat.object(new_oid).map_err(|e| vec![e])?;
            let (old_headers, old_message) = split_object(&old);
            let (new_headers, new_message) = split_object(&new);
            let get = |headers: &[Header], key: &[u8]| {
                headers
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| text(v))
                    .unwrap_or_default()
            };
            let old_object = get(&old_headers, b"object");
            let expected_object = self
                .finals
                .get(&old_object)
                .or_else(|| self.tag_finals.get(&old_object))
                .cloned()
                .unwrap_or(old_object);
            if get(&new_headers, b"object") != expected_object {
                errors.push(format!("検証: タグ {} が指すものが合わない", tag.name));
            }
            let mut ignored = Vec::new();
            let (expected, _) = self.transform(
                &format!("タグ {} の本文", tag.name),
                old_message,
                &mut ignored,
            );
            if expected != new_message {
                errors.push(format!("検証: タグ {} の本文が合わない", tag.name));
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
        &self,
        cat: &mut CatFile,
        old: &str,
        new: &str,
        prefix: &str,
        commit: &str,
        seen_trees: &mut HashSet<(String, String)>,
        seen_blobs: &mut HashSet<(String, String)>,
        errors: &mut Vec<String>,
    ) -> Result<(), String> {
        if !seen_trees.insert((old.to_string(), new.to_string())) {
            return Ok(());
        }
        let old_entries = parse_tree(&cat.object(old)?, self.hash_bytes);
        let new_entries = parse_tree(&cat.object(new)?, self.hash_bytes);
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
                    cat,
                    &o.oid,
                    &n.oid,
                    &format!("{path}/"),
                    commit,
                    seen_trees,
                    seen_blobs,
                    errors,
                )?,
                // サブモジュールのコミットは書き換えない
                "160000" => {
                    if o.oid != n.oid {
                        errors.push(format!("検証: コミット {commit} の {path} が変わった"));
                    }
                }
                _ => {
                    if !seen_blobs.insert((o.oid.clone(), n.oid.clone())) {
                        continue;
                    }
                    let old_data = cat.object(&o.oid)?;
                    let new_data = cat.object(&n.oid)?;
                    let mut ignored = Vec::new();
                    let (expected, _) = self.transform(&path, &old_data, &mut ignored);
                    if expected != new_data {
                        errors.push(format!("検証: コミット {commit} の {path} が合わない"));
                    }
                }
            }
        }
        Ok(())
    }
}

/// ブランチとタグを、書き換えた後のものに 1 つのトランザクションで移し、元の ref を残す
fn move_refs(
    top: &Path,
    refs: &[(String, String)],
    rewrite: &Rewrite,
    head: &Head,
) -> Result<(), Vec<String>> {
    let new_refs: HashMap<String, String> = list_new_refs(top).map_err(|e| vec![e])?;
    let mut errors = Vec::new();
    let mut commands = String::new();
    for (name, old) in refs {
        let rest = name.strip_prefix("refs/").unwrap_or(name);
        let new_name = format!("{NEW_NS}{rest}");
        let expected = rewrite
            .finals
            .get(old)
            .or_else(|| rewrite.tag_finals.get(old));
        match (expected, new_refs.get(&new_name)) {
            (Some(expected), Some(new)) if expected == new => {
                commands.push_str(&format!("update {name} {new} {old}\n"));
                commands.push_str(&format!("create {ORIGINAL_NS}{rest} {old}\n"));
                commands.push_str(&format!("delete {new_name} {new}\n"));
            }
            _ => errors.push(format!("検証: {name} の書き換えた後のものが合わない")),
        }
    }
    if let Head::Detached(old) = head {
        match rewrite.finals.get(old) {
            Some(new) => {
                commands.push_str(&format!("option no-deref\nupdate HEAD {new} {old}\n"));
            }
            None => errors.push(format!("HEAD が指す {old} を書き換えていない")),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    git_stdin(top, &["update-ref", "--stdin"], commands.as_bytes())
        .map(|_| ())
        .map_err(|e| vec![e])
}

fn list_new_refs(top: &Path) -> Result<HashMap<String, String>, String> {
    let out = git::git(
        top,
        &["for-each-ref", "--format=%(refname) %(objectname)", NEW_NS],
    )?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .filter_map(|line| line.split_once(' '))
        .map(|(name, oid)| (name.to_string(), oid.to_string()))
        .collect())
}

fn delete_new_refs(top: &Path) -> Result<(), String> {
    let refs = list_new_refs(top)?;
    let commands: String = refs
        .iter()
        .map(|(name, oid)| format!("delete {name} {oid}\n"))
        .collect();
    git_stdin(top, &["update-ref", "--stdin"], commands.as_bytes()).map(|_| ())
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

/// `git args…` に `input` を渡し、成功なら標準出力（末尾の改行を外す）を返す
fn git_stdin(top: &Path, args: &[&str], input: &[u8]) -> Result<String, String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(top)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("git を起動できない: {e}"))?;
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(input)
        .map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    let output = child
        .wait_with_output()
        .map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    if !output.status.success() {
        return Err(git::git_failure(args, &output));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string())
}

fn read_tag(name: &str, input: &mut Lines<impl BufRead>) -> Result<TagSpec, String> {
    let io = |e: std::io::Error| format!("fast-export の流れを読めない: {e}");
    let mut tag = TagSpec {
        name: name.to_string(),
        mark: None,
        from: String::new(),
        original: String::new(),
        tagger: Vec::new(),
        message: Vec::new(),
    };
    loop {
        let line = input.next().map_err(io)?.unwrap_or_default();
        let (key, value) = split_command(&line);
        match key {
            b"mark" => tag.mark = Some(text(value)),
            b"from" => tag.from = text(value),
            b"original-oid" => tag.original = text(value),
            b"tagger" => tag.tagger = line.clone(),
            b"data" => {
                tag.message = input.data(value).map_err(io)?;
                return Ok(tag);
            }
            _ => return Err(unexpected("tag", &line)),
        }
    }
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
    _child: Child,
    input: ChildStdin,
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
        let input = child.stdin.take().expect("piped stdin");
        let output = BufReader::new(child.stdout.take().expect("piped stdout"));
        Ok(CatFile {
            _child: child,
            input,
            output,
        })
    }

    fn object(&mut self, oid: &str) -> Result<Vec<u8>, String> {
        let io = |e: std::io::Error| format!("git cat-file: {e}");
        writeln!(self.input, "{oid}").map_err(io)?;
        self.input.flush().map_err(io)?;
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

/// コミットかタグのオブジェクトの見出しの行（キーと値）
type Header = (Vec<u8>, Vec<u8>);

/// コミットかタグのオブジェクトを、見出しの行（キーと値）と本文に分ける
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

fn is_hash(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 書き換えた履歴を置く ref の名前
fn new_ref(name: &str) -> String {
    format!("{NEW_NS}{}", name.strip_prefix("refs/").unwrap_or(name))
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
