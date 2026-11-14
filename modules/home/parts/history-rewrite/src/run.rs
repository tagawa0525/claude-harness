//! run: 元のリポジトリのすべてのブランチとタグを作業ディレクトリにコピーして書き換える（ADR-0045、ADR-0088）。
//!
//! 第 1 段は `git fast-export --no-data` の流れを読み、語を直したブロブと本文を `git fast-import` に渡す。
//! fast-export の設定は filter-repo 2.47.0 の fep_cmd に、ブロブを流さない `--no-data` と、署名つきのコミットの扱いを
//! 足したもの（コミットの署名は中身を変えると合わないので外す。照合は gpgsig の見出しを比べない）。流れはブロブと本文の
//! ほかを書き換えずに渡すので、コミットは消えず（filter-repo の `--prune-empty=never`、`--prune-degenerate=never`）、
//! 本文の番号は第 2 段だけが付け直し（`--preserve-commit-hashes`）、本文の符号化は変わらず（`--preserve-commit-encoding`）、
//! repack もしない（`--no-gc`）。replace の ref も足さない（`--replace-refs=delete-no-add`）。

use crate::{Stop, Transformer, Words, git};
use commit_refs::object::{CatFile, Header, split_command, split_object, strip_signature, values};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const MODE_LINK: &[u8] = b"120000";
const MODE_GITLINK: &[u8] = b"160000";
/// 照合で比べない、コミットの見出し。tree と parent は別に比べ、署名は書き換えで外れる
const SKIPPED_HEADERS: [&[u8]; 5] = [b"tree", b"parent", b"gpgsig", b"gpgsig-sha256", b"mergetag"];
/// filter-repo 2.47.0 の fep_cmd の設定（--all の代わりに、ブランチとタグを標準入力で渡す）に、--no-data と
/// --signed-commits=strip を足したもの
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
    "--stdin",
];
/// 直したブロブに振る印の始まり。fast-export は印を 1 から順に振るので、重ならない大きさから振る
const BLOB_MARKS: u64 = 1 << 40;
/// fast-export の --fake-missing-tagger が付ける仮の tagger（git 2.56.0 の fast-export.c で確かめた）
const FAKE_TAGGER: &[u8] = b"Unspecified Tagger <unspecified-tagger> 0 +0000";

/// 元の番号から新しい番号への対応表
pub type CommitMap = HashMap<String, String>;

/// filter-repo の commit-map の形（`old new` の見出しと、1 行に旧と新）を読む
pub fn read_map(path: &Path) -> Result<CommitMap, Stop> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| Stop(format!("{}: 読めない: {e}", path.display())))?;
    let lines: Vec<&str> = text.lines().collect();
    let has_header = lines
        .first()
        .is_some_and(|l| l.split_whitespace().eq(["old", "new"]));
    let (rows, first_number) = if has_header {
        (&lines[1..], 2)
    } else {
        (&lines[..], 1)
    };
    let mut map = CommitMap::new();
    for (k, line) in rows.iter().enumerate() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        match fields[..] {
            [] => continue,
            [old, new] => {
                map.insert(old.to_string(), new.to_string());
            }
            _ => {
                return Err(Stop(format!(
                    "{}:{}: 2 列でない: {line:?}",
                    path.display(),
                    first_number + k
                )));
            }
        }
    }
    Ok(map)
}

fn write_map(path: &Path, map: &CommitMap) -> Result<(), Stop> {
    let mut rows: Vec<(&String, &String)> = map.iter().collect();
    rows.sort();
    let mut text = String::from("old new\n");
    for (old, new) in rows {
        text.push_str(&format!("{old} {new}\n"));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Stop(format!("{}: {e}", dir.display())))?;
    }
    std::fs::write(path, text).map_err(|e| Stop(format!("{}: 書けない: {e}", path.display())))
}

/// コミットの encoding の見出しが無いか UTF-8 か（git の is_encoding_utf8 と同じく、大文字と小文字を問わずに utf-8 か
/// utf8）
pub fn is_utf8(encoding: Option<&[u8]>) -> bool {
    encoding.is_none_or(|e| e.eq_ignore_ascii_case(b"utf-8") || e.eq_ignore_ascii_case(b"utf8"))
}

/// git が encoding の見出しの名前で iconv に渡す符号化。WHATWG の名前（encoding_rs）で引き、引けなければ iconv と
/// Python の codecs の綴り（eucJP、euc_jp、cp932、latin1 など）を WHATWG の名前に読み替える
fn codec_of(label: &[u8]) -> Option<&'static encoding_rs::Encoding> {
    if let Some(found) = encoding_rs::Encoding::for_label(label) {
        return Some(found);
    }
    let folded: String = String::from_utf8_lossy(label)
        .to_ascii_lowercase()
        .chars()
        .filter(|c| !matches!(c, '-' | '_'))
        .collect();
    let whatwg = match folded.as_str() {
        "eucjp" => "euc-jp".to_string(),
        "shiftjis" | "sjis" => "shift_jis".to_string(),
        "cp932" | "ms932" | "windows31j" => "windows-31j".to_string(),
        "iso2022jp" => "iso-2022-jp".to_string(),
        "euckr" => "euc-kr".to_string(),
        "latin1" => "iso-8859-1".to_string(),
        f if f.starts_with("iso8859") => format!("iso-8859-{}", &f["iso8859".len()..]),
        f if f.len() > 2 && f.starts_with("cp") && f[2..].chars().all(|c| c.is_ascii_digit()) => {
            format!("windows-{}", &f[2..])
        }
        _ => return None,
    };
    encoding_rs::Encoding::for_label(whatwg.as_bytes())
}

/// 宣言した符号化 encoding（UTF-8 でない）の本文に、直す語があるか
fn message_would_change(
    transformer: &Transformer,
    data: &[u8],
    encoding: &[u8],
    name: &str,
) -> Result<bool, Stop> {
    let label = String::from_utf8_lossy(encoding);
    let Some(codec) = codec_of(encoding) else {
        return Err(Stop(format!("{name}: 符号化 {label} を知らない")));
    };
    let Some(text) = codec.decode_without_bom_handling_and_without_replacement(data) else {
        return Err(Stop(format!(
            "{name}: 符号化 {label} として読めない本文がある"
        )));
    };
    Ok(transformer.text(name, &text, true)? != text)
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// fast-export のパス（`"` で囲んだものは C の書き方のエスケープ）を、バイト列のパスにする（git の quote.c の
/// unquote_c_style）
fn unquote(path: &[u8]) -> Vec<u8> {
    let Some(inner) = path.strip_prefix(b"\"").and_then(|p| p.strip_suffix(b"\"")) else {
        return path.to_vec();
    };
    let mut out = Vec::with_capacity(inner.len());
    let mut i = 0;
    while i < inner.len() {
        let b = inner[i];
        i += 1;
        if b != b'\\' || i >= inner.len() {
            out.push(b);
            continue;
        }
        let e = inner[i];
        i += 1;
        match e {
            b'a' => out.push(0x07),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(0x0b),
            // \ooo の 8 進（git は 0〜3 で始まる 3 桁で書く）
            b'0'..=b'3' if i + 1 < inner.len() => {
                let digits = [e, inner[i], inner[i + 1]];
                let value = digits
                    .iter()
                    .fold(0u32, |acc, d| acc * 8 + u32::from(d - b'0'));
                out.push(value as u8);
                i += 2;
            }
            other => out.push(other),
        }
    }
    out
}

/// 1 行ずつ読み、次の行を覗ける読み手。`data` の中身はバイト数で読む
struct Lines<R> {
    reader: R,
    peeked: Option<Vec<u8>>,
}

impl<R: BufRead> Lines<R> {
    fn read(&mut self) -> Result<Option<Vec<u8>>, Stop> {
        let mut line = Vec::new();
        if self.reader.read_until(b'\n', &mut line).map_err(io)? == 0 {
            return Ok(None);
        }
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        Ok(Some(line))
    }

    fn next(&mut self) -> Result<Option<Vec<u8>>, Stop> {
        match self.peeked.take() {
            Some(line) => Ok(Some(line)),
            None => self.read(),
        }
    }

    fn peek(&mut self) -> Result<Option<&Vec<u8>>, Stop> {
        if self.peeked.is_none() {
            self.peeked = self.read()?;
        }
        Ok(self.peeked.as_ref())
    }

    /// `data <n>` の後の n バイト。後に続く省いてよい改行も読み飛ばす
    fn data(&mut self, count: &[u8]) -> Result<Vec<u8>, Stop> {
        let count: usize = text(count).parse().map_err(|_| {
            Stop(format!(
                "fast-export の data の長さが読めない: {}",
                text(count)
            ))
        })?;
        let mut data = vec![0; count];
        self.reader.read_exact(&mut data).map_err(io)?;
        if self.reader.fill_buf().map_err(io)?.first() == Some(&b'\n') {
            self.reader.consume(1);
        }
        Ok(data)
    }
}

fn io(e: std::io::Error) -> Stop {
    Stop(format!(
        "git fast-export と fast-import とのやりとりに失敗した: {e}"
    ))
}

fn write_data(out: &mut impl Write, data: &[u8]) -> Result<(), Stop> {
    writeln!(out, "data {}", data.len()).map_err(io)?;
    out.write_all(data).map_err(io)?;
    out.write_all(b"\n").map_err(io)
}

/// 第 1 段の数
pub struct Counts {
    /// 語を直したブロブの数（パスとブロブの組ごと）
    pub blobs: usize,
    /// 語を直したコミットとタグの本文の数
    pub messages: usize,
}

/// 第 1 段: ブランチとタグの流れに語の変換だけを当てる。対応表（元の番号 → 第 1 段の番号）を返す
pub fn stage1(repo: &Path, transformer: &mut Transformer) -> Result<(Counts, CommitMap), Stop> {
    let refs = git(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/heads/",
            "refs/tags/",
        ],
    )?;
    let marks_file = repo.join(".git").join("history-rewrite").join("marks");
    std::fs::create_dir_all(marks_file.parent().unwrap())
        .map_err(|e| Stop(format!("{}: {e}", marks_file.display())))?;
    let mut export = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(FAST_EXPORT)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| Stop(format!("git fast-export を起動できない: {e}")))?;
    export
        .stdin
        .take()
        .expect("piped")
        .write_all(&refs)
        .map_err(io)?;
    let mut import = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "fast-import",
            "--force",
            "--quiet",
            "--date-format=raw-permissive",
        ])
        .arg(format!("--export-marks={}", marks_file.display()))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .map_err(|e| Stop(format!("git fast-import を起動できない: {e}")))?;
    let mut cat = CatFile::new(repo).map_err(Stop)?;
    let mut input = Lines {
        reader: BufReader::new(export.stdout.take().expect("piped")),
        peeked: None,
    };
    let mut output = std::io::BufWriter::new(import.stdin.take().expect("piped"));
    let mut filter = Filter {
        transformer,
        cat: &mut cat,
        seen: HashMap::new(),
        next_mark: BLOB_MARKS,
        counts: Counts {
            blobs: 0,
            messages: 0,
        },
        originals: HashMap::new(),
        ended_early: false,
    };
    let result = filter.stream(&mut input, &mut output);
    let (counts, originals, ended_early) = (filter.counts, filter.originals, filter.ended_early);
    if result.is_err() {
        // 途中で閉じた流れを fast-import に読ませると「stream ends early」と crash report を書き、止めた理由が埋もれる
        let _ = import.kill();
        if !ended_early {
            let _ = export.kill();
        }
    }
    drop(output);
    let export_status = export.wait().map_err(io)?;
    let import_status = import.wait().map_err(io)?;
    if ended_early && !export_status.success() {
        // 流れが終わったのは fast-export が失敗したため（理由は fast-export の標準エラー）
        return Err(Stop(format!(
            "git fast-export が失敗した（{export_status}）"
        )));
    }
    result?;
    if !export_status.success() {
        return Err(Stop(format!(
            "git fast-export が失敗した（{export_status}）"
        )));
    }
    if !import_status.success() {
        return Err(Stop(format!(
            "git fast-import が失敗した（{import_status}）"
        )));
    }
    let marks = std::fs::read_to_string(&marks_file)
        .map_err(|e| Stop(format!("{}: 読めない: {e}", marks_file.display())))?;
    let new_of: HashMap<&str, &str> = marks.lines().filter_map(|l| l.split_once(' ')).collect();
    let mut map = CommitMap::new();
    for (mark, original) in originals {
        let Some(new) = new_of.get(mark.as_str()) else {
            return Err(Stop(format!(
                "fast-import が印 {mark}（{original}）を書き出していない"
            )));
        };
        map.insert(original, new.to_string());
    }
    // fast-import は ref を動かすが作業ツリーは変えないので、チェックアウトしたブランチの新しい中身にする
    git(repo, &["reset", "-q", "--hard"])?;
    write_map(
        &repo.join(".git").join("history-rewrite").join("commit-map"),
        &map,
    )?;
    Ok((counts, map))
}

struct Filter<'a> {
    transformer: &'a mut Transformer,
    cat: &'a mut CatFile,
    /// （パス、元のブロブ）→ 直したブロブの印（直さなければ None）
    seen: HashMap<(Vec<u8>, Vec<u8>), Option<u64>>,
    next_mark: u64,
    counts: Counts,
    /// コミットの印 → 元の番号
    originals: HashMap<String, String>,
    /// fast-export の流れが done の前に終わった
    ended_early: bool,
}

impl Filter<'_> {
    fn stream(
        &mut self,
        input: &mut Lines<impl BufRead>,
        out: &mut impl Write,
    ) -> Result<(), Stop> {
        loop {
            let Some(line) = input.next()? else {
                self.ended_early = true;
                return Err(Stop("fast-export の流れが done の前に終わった".to_string()));
            };
            let (command, _) = split_command(&line);
            match command {
                b"commit" => self.commit(&line, input, out)?,
                b"tag" => self.tag(&line, input, out)?,
                b"done" => {
                    out.write_all(b"done\n").map_err(io)?;
                    return Ok(());
                }
                // reset とその from、feature done、空の行は、そのまま渡す
                _ => {
                    out.write_all(&line).map_err(io)?;
                    out.write_all(b"\n").map_err(io)?;
                }
            }
        }
    }

    fn commit(
        &mut self,
        first: &[u8],
        input: &mut Lines<impl BufRead>,
        out: &mut impl Write,
    ) -> Result<(), Stop> {
        let mut header: Vec<Vec<u8>> = vec![first.to_vec()];
        let mut mark = String::new();
        let mut original = String::new();
        let mut encoding: Option<Vec<u8>> = None;
        let message = loop {
            let Some(line) = input.next()? else {
                return Err(Stop("fast-export の commit が途中で終わった".to_string()));
            };
            let (key, value) = split_command(&line);
            match key {
                b"mark" => mark = text(value),
                b"original-oid" => original = text(value),
                b"encoding" => encoding = Some(value.to_vec()),
                b"data" => break input.data(value)?,
                b"author" | b"committer" => {}
                _ => {
                    return Err(Stop(format!(
                        "fast-export の commit で知らない行: {}",
                        text(&line)
                    )));
                }
            }
            header.push(line);
        };
        let name = format!("コミット {original} の本文");
        let message = if is_utf8(encoding.as_deref()) {
            let new = self.transformer.message(&message, &name)?;
            if new != message {
                self.counts.messages += 1;
            }
            new
        } else {
            // 本文は宣言した符号化で読まれる。UTF-8 の語を書き込むと文字化けするので、変えずに止める
            let encoding = encoding.unwrap_or_default();
            if message_would_change(self.transformer, &message, &encoding, &name)? {
                return Err(Stop(format!(
                    "{original}: 符号化 {} の本文に直す語がある（UTF-8 の本文だけを直す）",
                    text(&encoding)
                )));
            }
            message
        };
        let mut ops: Vec<Vec<u8>> = Vec::new();
        let mut blobs: Vec<(u64, Vec<u8>)> = Vec::new();
        while let Some(line) = input.peek()? {
            if line.is_empty() {
                input.next()?;
                break;
            }
            let (key, _) = split_command(line);
            if !matches!(
                key,
                b"from" | b"merge" | b"M" | b"D" | b"C" | b"R" | b"deleteall"
            ) {
                break;
            }
            let line = input.next()?.unwrap_or_default();
            let (key, value) = split_command(&line);
            if key != b"M" {
                ops.push(line);
                continue;
            }
            let mut parts = value.splitn(3, |b| *b == b' ');
            let (Some(mode), Some(oid), Some(path)) = (parts.next(), parts.next(), parts.next())
            else {
                return Err(Stop(format!(
                    "fast-export の commit で知らない行: {}",
                    text(&line)
                )));
            };
            if mode == MODE_LINK || mode == MODE_GITLINK {
                ops.push(line);
                continue;
            }
            let real_path = unquote(path);
            let key = (real_path.clone(), oid.to_vec());
            let new_mark = match self.seen.get(&key) {
                Some(found) => *found,
                None => {
                    let data = self.cat.object(&text(oid)).map_err(Stop)?;
                    let changed = self.transformer.blob(&real_path, &data)?;
                    let mark = changed.map(|content| {
                        let mark = self.next_mark;
                        self.next_mark += 1;
                        self.counts.blobs += 1;
                        blobs.push((mark, content));
                        mark
                    });
                    self.seen.insert(key, mark);
                    mark
                }
            };
            match new_mark {
                Some(m) => {
                    let mut op = format!("M {} :{m} ", text(mode)).into_bytes();
                    op.extend_from_slice(path);
                    ops.push(op);
                }
                None => ops.push(line),
            }
        }
        // 直したブロブは、使うコミットより前に流す（fast-import はコミットの中でブロブを作らない）
        for (mark, content) in blobs {
            write!(out, "blob\nmark :{mark}\n").map_err(io)?;
            write_data(out, &content)?;
        }
        for line in &header {
            out.write_all(line).map_err(io)?;
            out.write_all(b"\n").map_err(io)?;
        }
        write_data(out, &message)?;
        for op in &ops {
            out.write_all(op).map_err(io)?;
            out.write_all(b"\n").map_err(io)?;
        }
        out.write_all(b"\n").map_err(io)?;
        if !mark.is_empty() {
            self.originals.insert(mark, original);
        }
        Ok(())
    }

    fn tag(
        &mut self,
        first: &[u8],
        input: &mut Lines<impl BufRead>,
        out: &mut impl Write,
    ) -> Result<(), Stop> {
        let name = text(split_command(first).1);
        out.write_all(first).map_err(io)?;
        out.write_all(b"\n").map_err(io)?;
        loop {
            let Some(line) = input.next()? else {
                return Err(Stop("fast-export の tag が途中で終わった".to_string()));
            };
            let (key, value) = split_command(&line);
            match key {
                b"mark" | b"from" | b"original-oid" | b"tagger" => {
                    out.write_all(&line).map_err(io)?;
                    out.write_all(b"\n").map_err(io)?;
                }
                b"data" => {
                    let message = input.data(value)?;
                    let new = self
                        .transformer
                        .message(&message, &format!("タグ {name} の本文"))?;
                    if new != message {
                        self.counts.messages += 1;
                    }
                    return write_data(out, &new);
                }
                _ => {
                    return Err(Stop(format!(
                        "fast-export の tag で知らない行: {}",
                        text(&line)
                    )));
                }
            }
        }
    }
}

/// 元の履歴（ミラーのオブジェクトを alternates で見る）と第 1 段の結果を突き合わせる
pub struct Verifier<'a> {
    rewritten: PathBuf,
    original: PathBuf,
    mapping: &'a CommitMap,
    transformer: &'a mut Transformer,
    problems: Vec<String>,
    /// 照合したブロブの組（パス、元、新）
    verified_blobs: HashSet<(Vec<u8>, String, String)>,
    verified_trees: HashSet<(String, String)>,
}

impl<'a> Verifier<'a> {
    pub fn new(
        rewritten: &Path,
        original: &Path,
        mapping: &'a CommitMap,
        transformer: &'a mut Transformer,
    ) -> Self {
        Verifier {
            rewritten: rewritten.to_path_buf(),
            original: original.to_path_buf(),
            mapping,
            transformer,
            problems: Vec::new(),
            verified_blobs: HashSet::new(),
            verified_trees: HashSet::new(),
        }
    }

    pub fn run(mut self) -> Result<(), Stop> {
        let mut cat = CatFile::with_alternate(&self.rewritten, &self.original.join("objects"))
            .map_err(Stop)?;
        // filter-repo に渡すのはブランチとタグだけなので、ミラーに写った refs/replace/ には従わない
        let originals = git(
            &self.original,
            &["--no-replace-objects", "rev-list", "--branches", "--tags"],
        )?;
        let originals: Vec<String> = text(&originals)
            .split_whitespace()
            .map(str::to_string)
            .collect();
        let mut missing: Vec<&String> = originals
            .iter()
            .filter(|o| !self.mapping.contains_key(*o))
            .collect();
        missing.sort();
        if !missing.is_empty() {
            let missing: Vec<&str> = missing.iter().map(|s| s.as_str()).collect();
            self.problems.push(format!(
                "commit-map に無い元のコミット: {}",
                missing.join(" ")
            ));
        }
        let mut pruned: Vec<&String> = self
            .mapping
            .iter()
            .filter(|(_, new)| new.chars().all(|c| c == '0'))
            .map(|(old, _)| old)
            .collect();
        pruned.sort();
        if !pruned.is_empty() {
            let pruned: Vec<&str> = pruned.iter().map(|s| s.as_str()).collect();
            self.problems
                .push(format!("第 1 段でコミットが消えた: {}", pruned.join(" ")));
        }
        // 番号の変わらないコミットも確かめる（変換が中身を変えるはずなのに当てなかったものを見る）
        let mut pairs: Vec<(String, String)> = self
            .mapping
            .iter()
            .filter(|(_, new)| !new.chars().all(|c| c == '0'))
            .map(|(o, n)| (o.clone(), n.clone()))
            .collect();
        pairs.sort();
        for (old, new) in &pairs {
            self.commit(&mut cat, old, new)?;
        }
        self.refs(&mut cat)?;
        if self.problems.is_empty() {
            return Ok(());
        }
        let shown: Vec<&str> = self.problems.iter().take(50).map(String::as_str).collect();
        Err(Stop(format!(
            "照合が合わない（{} 件）:\n{}",
            self.problems.len(),
            shown.join("\n")
        )))
    }

    fn commit(&mut self, cat: &mut CatFile, old: &str, new: &str) -> Result<(), Stop> {
        let old_data = cat.object(old).map_err(Stop)?;
        let new_data = cat.object(new).map_err(Stop)?;
        let (old_headers, old_body) = split_object(&old_data);
        let (new_headers, new_body) = split_object(&new_data);
        let kept = |headers: &[Header]| -> Vec<Header> {
            headers
                .iter()
                .filter(|(k, _)| !SKIPPED_HEADERS.contains(&k.as_slice()))
                .cloned()
                .collect()
        };
        let (old_kept, new_kept) = (kept(&old_headers), kept(&new_headers));
        if old_kept != new_kept {
            let a: HashSet<&Header> = old_kept.iter().collect();
            let b: HashSet<&Header> = new_kept.iter().collect();
            let mut differ: Vec<String> =
                a.symmetric_difference(&b).map(|(k, _)| text(k)).collect();
            differ.sort();
            differ.dedup();
            // fast-export は知らない見出し（jj の change-id など）を書き出さないので、その名前を示す
            self.problems
                .push(format!("{old}: 見出しが変わった: {}", differ.join(", ")));
        }
        let old_parents: Vec<String> = values(&old_headers, b"parent")
            .iter()
            .map(|v| text(v))
            .collect();
        let new_parents: Vec<String> = values(&new_headers, b"parent")
            .iter()
            .map(|v| text(v))
            .collect();
        let mapped: Vec<String> = old_parents
            .iter()
            .map(|p| self.mapping.get(p).cloned().unwrap_or_else(|| p.clone()))
            .collect();
        if mapped != new_parents {
            self.problems.push(format!(
                "{old}: 親が対応しない: {old_parents:?} → {new_parents:?}"
            ));
        }
        // 宣言した符号化が UTF-8 でない本文は変えない（変わるなら第 1 段が止める）
        let encoding = values(&old_headers, b"encoding").into_iter().next();
        let want = if is_utf8(encoding.as_deref()) {
            self.transformer
                .message(old_body, &format!("コミット {old} の本文"))?
        } else {
            old_body.to_vec()
        };
        if want != new_body {
            self.problems.push(format!("{old}: 本文が変換の結果と違う"));
        }
        let trees = (
            text(&values(&old_headers, b"tree").concat()),
            text(&values(&new_headers, b"tree").concat()),
        );
        // 同じ tree の組は履歴の中で何度も出る（語を直さないコミット、マージ）ので、1 度だけ比べる
        if self.verified_trees.insert(trees) {
            self.tree(cat, old, new)?;
        }
        Ok(())
    }

    fn tree(&mut self, cat: &mut CatFile, old: &str, new: &str) -> Result<(), Stop> {
        let (old_tree, new_tree) = (self.ls_tree(old)?, self.ls_tree(new)?);
        let mut paths: Vec<&Vec<u8>> = old_tree.keys().chain(new_tree.keys()).collect();
        paths.sort();
        paths.dedup();
        for path in paths {
            let shown = format!("{:?}", text(path));
            let (Some(o), Some(n)) = (old_tree.get(path), new_tree.get(path)) else {
                let what = if old_tree.contains_key(path) {
                    "消えた"
                } else {
                    "増えた"
                };
                self.problems.push(format!("{old}: {shown} が{what}"));
                continue;
            };
            if o.mode != n.mode {
                self.problems
                    .push(format!("{old}: {shown} のモードが変わった"));
                continue;
            }
            let is_blob = o.kind == "blob" && o.mode != "120000" && o.mode != "160000";
            if !is_blob {
                if o.oid != n.oid {
                    self.problems
                        .push(format!("{old}: {shown} が変換の結果と違う"));
                }
                continue;
            }
            let key = (path.clone(), o.oid.clone(), n.oid.clone());
            if self.verified_blobs.contains(&key) {
                continue;
            }
            let content = cat.object(&o.oid).map_err(Stop)?;
            let ok = match self.transformer.blob(path, &content)? {
                None => o.oid == n.oid,
                Some(want) => cat.object(&n.oid).map_err(Stop)? == want,
            };
            if ok {
                self.verified_blobs.insert(key);
            } else {
                self.problems
                    .push(format!("{old}: {shown} が変換の結果と違う"));
            }
        }
        Ok(())
    }

    fn ls_tree(&self, commit: &str) -> Result<HashMap<Vec<u8>, Entry>, Stop> {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.rewritten)
            .args(["ls-tree", "-r", "-z", "--full-tree", commit])
            .env(
                "GIT_ALTERNATE_OBJECT_DIRECTORIES",
                self.original.join("objects"),
            )
            .output()
            .map_err(|e| Stop(format!("git ls-tree を起動できない: {e}")))?;
        if !out.status.success() {
            return Err(Stop(format!(
                "git ls-tree {commit}: {}",
                text(&out.stderr).trim()
            )));
        }
        let mut entries = HashMap::new();
        for record in out.stdout.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            let Some(tab) = record.iter().position(|b| *b == b'\t') else {
                return Err(Stop(format!("git ls-tree の知らない行: {}", text(record))));
            };
            let meta = text(&record[..tab]);
            let mut fields = meta.split(' ');
            let (Some(mode), Some(kind), Some(oid)) = (fields.next(), fields.next(), fields.next())
            else {
                return Err(Stop(format!("git ls-tree の知らない行: {}", text(record))));
            };
            entries.insert(
                record[tab + 1..].to_vec(),
                Entry {
                    mode: mode.to_string(),
                    kind: kind.to_string(),
                    oid: oid.to_string(),
                },
            );
        }
        Ok(entries)
    }

    fn refs(&mut self, cat: &mut CatFile) -> Result<(), Stop> {
        let old_refs = for_each_ref(&self.original)?;
        let new_refs = for_each_ref(&self.rewritten)?;
        let old_names: HashSet<&String> = old_refs.keys().collect();
        let new_names: HashSet<&String> = new_refs.keys().collect();
        if old_names != new_names {
            let mut differ: Vec<&&String> = old_names.symmetric_difference(&new_names).collect();
            differ.sort();
            self.problems
                .push(format!("ブランチとタグの名前が合わない: {differ:?}"));
        }
        let mut names: Vec<&&String> = old_names.intersection(&new_names).collect();
        names.sort();
        for name in names {
            let ((old_oid, kind), (new_oid, _)) = (&old_refs[*name], &new_refs[*name]);
            match kind.as_str() {
                "commit" => {
                    if self.mapping.get(old_oid) != Some(new_oid) {
                        self.problems.push(format!("{name}: 対応表の新を指さない"));
                    }
                }
                "tag" => {
                    let want = self.tag(cat, old_oid)?;
                    if cat.object(new_oid).map_err(Stop)? != want {
                        self.problems
                            .push(format!("{name}: 注釈つきのタグが変換の結果と違う"));
                    }
                }
                other => self
                    .problems
                    .push(format!("{name}: コミットとタグのほか（{other}）を指す")),
            }
        }
        Ok(())
    }

    /// 注釈つきのタグを、第 1 段が作るはずのオブジェクトにしたもの
    fn tag(&mut self, cat: &mut CatFile, oid: &str) -> Result<Vec<u8>, Stop> {
        let data = cat.object(oid).map_err(Stop)?;
        let (mut headers, body) = split_object(&data);
        if !headers.iter().any(|(k, _)| k == b"tagger") {
            // fast-export の --fake-missing-tagger は、tag の見出しの後に仮の tagger を付ける
            let at = headers
                .iter()
                .position(|(k, _)| k == b"tag")
                .map_or(headers.len(), |p| p + 1);
            headers.insert(at, (b"tagger".to_vec(), FAKE_TAGGER.to_vec()));
        }
        let is_tag_of_tag = values(&headers, b"type")
            .first()
            .is_some_and(|t| t == b"tag");
        let mut content = Vec::new();
        for (key, value) in &headers {
            let value = if key == b"object" {
                if is_tag_of_tag {
                    return Err(Stop(format!(
                        "{oid}: タグを指すタグ（run の初めに止めるはずのもの）"
                    )));
                }
                let target = text(value);
                self.mapping
                    .get(&target)
                    .cloned()
                    .unwrap_or(target)
                    .into_bytes()
            } else {
                value.clone()
            };
            content.extend_from_slice(key);
            content.push(b' ');
            content.extend_from_slice(&value);
            content.push(b'\n');
        }
        content.push(b'\n');
        content.extend(
            self.transformer
                .message(strip_signature(body), &format!("タグ {oid} の本文"))?,
        );
        Ok(content)
    }
}

struct Entry {
    mode: String,
    kind: String,
    oid: String,
}

/// refs/heads/ と refs/tags/ の名前 → （指すオブジェクト、その種類）
fn for_each_ref(repo: &Path) -> Result<HashMap<String, (String, String)>, Stop> {
    let out = git(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname) %(objecttype)",
            "refs/heads/",
            "refs/tags/",
        ],
    )?;
    let mut refs = HashMap::new();
    for line in text(&out).lines() {
        let mut parts = line.split(' ');
        if let (Some(name), Some(oid), Some(kind)) = (parts.next(), parts.next(), parts.next()) {
            refs.insert(name.to_string(), (oid.to_string(), kind.to_string()));
        }
    }
    Ok(refs)
}

/// タグを指すタグがあれば止める（fast-export は外のタグの名前で内のタグを作り直すので、照合できる形でない）
fn refuse_nested_tags(repo: &Path) -> Result<(), Stop> {
    let out = git(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname) %(objecttype) %(type)",
            "refs/tags/",
        ],
    )?;
    let nested: Vec<String> = text(&out)
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split(' ').collect();
            (fields.get(1..) == Some(&["tag", "tag"][..])).then(|| fields[0].to_string())
        })
        .collect();
    if nested.is_empty() {
        return Ok(());
    }
    let hint: Vec<String> = nested
        .iter()
        .map(|n| {
            let short = n.strip_prefix("refs/tags/").unwrap_or(n);
            format!("git tag -f {short} {short}^{{}}")
        })
        .collect();
    Err(Stop(format!(
        "タグを指すタグがある: {}（コミットを指すタグに直してから: {}）",
        nested.join(" "),
        hint.join("、")
    )))
}

/// path の実体のパス。無い末尾の成分は、いちばん近い、ある祖先の実体のパスにつなぐ（無くても止めない）
fn resolve(path: &Path) -> Result<PathBuf, Stop> {
    let absolute =
        std::path::absolute(path).map_err(|e| Stop(format!("{}: {e}", path.display())))?;
    let mut rest = Vec::new();
    let mut cur = absolute.as_path();
    loop {
        if let Ok(real) = std::fs::canonicalize(cur) {
            return Ok(rest
                .iter()
                .rev()
                .fold(real, |acc: PathBuf, part| acc.join(part)));
        }
        match (cur.parent(), cur.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_os_string());
                cur = parent;
            }
            _ => return Ok(absolute),
        }
    }
}

/// run の本体。出力は main.rs の冒頭
pub fn run(words: &Words, source: &Path, workdir: &Path) -> Result<(), Stop> {
    let mut transformer = Transformer::new(words)?;
    let source =
        std::fs::canonicalize(source).map_err(|e| Stop(format!("{}: {e}", source.display())))?;
    let workdir = resolve(workdir)?;
    if workdir.starts_with(&source) {
        return Err(Stop(format!(
            "{} が {} の中（元のリポジトリの外に置く）",
            workdir.display(),
            source.display()
        )));
    }
    if workdir.exists() && !workdir.is_dir() {
        return Err(Stop(format!("{} がディレクトリでない", workdir.display())));
    }
    if workdir.is_dir()
        && std::fs::read_dir(&workdir)
            .map_err(|e| Stop(format!("{}: {e}", workdir.display())))?
            .next()
            .is_some()
    {
        return Err(Stop(format!(
            "{} が空でない（前の書き換えを確かめて片付けてから）",
            workdir.display()
        )));
    }
    std::fs::create_dir_all(&workdir).map_err(|e| Stop(format!("{}: {e}", workdir.display())))?;
    let original = workdir.join("original.git");
    let rewritten = workdir.join("rewritten");
    let (original_s, rewritten_s, source_s) = (
        original.display().to_string(),
        rewritten.display().to_string(),
        source.display().to_string(),
    );

    git(
        &workdir,
        &["clone", "-q", "--mirror", &source_s, &original_s],
    )?;
    let head = Command::new("git")
        .arg("-C")
        .arg(&original)
        .args(["symbolic-ref", "-q", "HEAD"])
        .output()
        .map_err(|e| Stop(format!("git を起動できない: {e}")))?;
    if !head.status.success() {
        return Err(Stop(format!(
            "{source_s} の HEAD がブランチを指していない（ブランチに移ってから）"
        )));
    }
    let head = text(&head.stdout).trim().to_string();
    if git(&original, &["for-each-ref", &head])?.is_empty() {
        return Err(Stop(format!(
            "{source_s} の HEAD のブランチ {head} にコミットが無い"
        )));
    }
    git(&workdir, &["init", "-q", &rewritten_s])?;
    git(
        &rewritten,
        &[
            "fetch",
            "-q",
            "--no-tags",
            // 作ったばかりのリポジトリの HEAD（まだコミットの無いブランチ）にも取り込む
            "--update-head-ok",
            &original_s,
            "+refs/heads/*:refs/heads/*",
            "+refs/tags/*:refs/tags/*",
        ],
    )?;
    git(&rewritten, &["symbolic-ref", "HEAD", &head])?;
    git(&rewritten, &["reset", "-q", "--hard"])?;
    refuse_nested_tags(&rewritten)?;

    let (counts, stage1_map) = stage1(&rewritten, &mut transformer)?;
    // 照合も同じブロブを読み直して数えるので、第 1 段の数をここで取る
    let not_utf8 = transformer.not_utf8;
    Verifier::new(&rewritten, &original, &stage1_map, &mut transformer).run()?;

    println!("ORIGINAL: {original_s}");
    println!("REWRITTEN: {rewritten_s}");
    println!("COMMITS: {}", stage1_map.len());
    println!("BLOBS: {}", counts.blobs);
    println!("MESSAGES: {}", counts.messages);
    println!("NOT_UTF8: {not_utf8}");
    std::io::stdout().flush().map_err(|e| Stop(e.to_string()))?;

    let stage1_path = rewritten
        .join(".git")
        .join("history-rewrite")
        .join("commit-map");
    let status = Command::new("commit-refs")
        .arg("rewrite-history")
        .arg(&stage1_path)
        .current_dir(&rewritten)
        .status()
        .map_err(|e| {
            Stop(format!(
                "commit-refs を起動できない（PATH に commit-refs が無い）: {e}"
            ))
        })?;
    if !status.success() {
        return Err(Stop(
            "commit-refs rewrite-history が止まった（理由は上）".to_string(),
        ));
    }
    let second = read_map(
        &rewritten
            .join(".git")
            .join("commit-refs")
            .join("commit-map"),
    )?;
    let last: CommitMap = stage1_map
        .iter()
        .map(|(old, mid)| {
            (
                old.clone(),
                second.get(mid).cloned().unwrap_or_else(|| mid.clone()),
            )
        })
        .collect();
    let map_path = workdir.join("commit-map");
    write_map(&map_path, &last)?;
    println!("MAP: {}", map_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoted_paths_are_unquoted_like_git() {
        assert_eq!(unquote(b"plain name.md"), b"plain name.md");
        assert_eq!(unquote(br#""a\tb\"c\\d""#), b"a\tb\"c\\d");
        assert_eq!(unquote(br#""\343\201\202.md""#), "あ.md".as_bytes());
        assert_eq!(unquote(br#""new\nline""#), b"new\nline");
    }

    #[test]
    fn utf8_encodings_ignore_case_and_dashes() {
        assert!(is_utf8(None));
        assert!(is_utf8(Some(b"UTF-8")));
        assert!(is_utf8(Some(b"utf8")));
        assert!(!is_utf8(Some(b"ISO-8859-1")));
        // git の is_encoding_utf8 は utf-8 と utf8 の 2 つの綴りだけを UTF-8 とする
        assert!(!is_utf8(Some(b"u-t-f-8")));
        assert!(!is_utf8(Some(b"UTF--8")));
    }
}
