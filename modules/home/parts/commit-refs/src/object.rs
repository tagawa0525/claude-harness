//! git のオブジェクトの読み方（`git cat-file --batch`）と、コミットとタグのオブジェクトの分け方とタグの署名の外し方。
//! rewrite-history の照合が使う。Rust に移す history-rewrite の照合も同じ関数を呼べるよう、ライブラリに置く（ADR-0088）

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

/// `git cat-file --batch` で、オブジェクトを 1 つずつ読む
pub struct CatFile {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}

impl CatFile {
    pub fn new(top: &Path) -> Result<Self, String> {
        Self::spawn(Command::new("git").arg("-C").arg(top))
    }

    /// 別のリポジトリのオブジェクトも読む（`GIT_ALTERNATE_OBJECT_DIRECTORIES` に objects を渡す。照合は書き換える前の
    /// オブジェクトをミラーから読み、書き換えたリポジトリに ref を足さない）
    pub fn with_alternate(top: &Path, objects: &Path) -> Result<Self, String> {
        Self::spawn(
            Command::new("git")
                .arg("-C")
                .arg(top)
                .env("GIT_ALTERNATE_OBJECT_DIRECTORIES", objects),
        )
    }

    fn spawn(command: &mut Command) -> Result<Self, String> {
        let mut child = command
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

    pub fn object(&mut self, oid: &str) -> Result<Vec<u8>, String> {
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
pub type Header = (Vec<u8>, Vec<u8>);

/// コミットかタグのオブジェクトを、見出しの行と本文に分ける
pub fn split_object(data: &[u8]) -> (Vec<Header>, &[u8]) {
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

pub fn values(headers: &[Header], key: &[u8]) -> Vec<Vec<u8>> {
    headers
        .iter()
        .filter(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .collect()
}

/// タグの本文の末尾の署名を外す。git の parse_signed_buffer と同じく、署名の開始マーカーで始まる行のうち最後の
/// 行から後を署名とする。中身を変えると署名は合わないので、fast-export の --signed-tags=strip と同じく外す
pub fn strip_signature(message: &[u8]) -> &[u8] {
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

/// 行を、最初の空白の前（命令やキー）と後に分ける
pub fn split_command(line: &[u8]) -> (&[u8], &[u8]) {
    match line.iter().position(|b| *b == b' ') {
        Some(p) => (&line[..p], &line[p + 1..]),
        None => (line, &[]),
    }
}
