//! `wait-step.sh <自分の worktree のパス> [<background の出力のファイル>...]` / `wait-step.sh --end <自分の worktree のパス>`:
//! /delegate の担当が、ターンを終えずに待つ 1 歩（ADR-0082 で sh から移した。`~/.claude/scripts/wait-step.sh` が呼ぶ）。
//!
//! 担当は、待つ間これをフォアグラウンドで続けて呼ぶ。ターンを終えると、そのたびに親に通知が届き親が応答するので、終えずに
//! 待つ。1 回の呼び出しは約 4 分 30 秒で返るので、呼ぶたびの応答で担当のキャッシュ（5 分で切れる）が保たれる。待つ間に届いた
//! 通知（/code-review の終わり、親の返事など）は、呼び出しが返った後に担当に渡る。
//!
//! 時計は担当ごとに 1 つで、worktree のパスで分ける（並行する担当どうしで、リポジトリやブランチの名前が重なっても取り合わ
//! ない）。親に最後に途中の報告を送った時刻（初めは待ち始め）と、最後に呼んだ時刻を `<一時のディレクトリ>/claude-wait-step/<鍵の
//! ハッシュ>` に残す。途中の報告から 55 分を超えたら REPORT を出して数え直す（担当は親に 1 通送って待ちを続ける。親のキャッシュの
//! 1 時間が切れる前に、親に 1 回だけ応答させる。止めるかは親が決める）。前の呼び出しから 55 分を超えて空いていれば、終わった
//! 待ち（--end の忘れ、中断した担当）の時計なので数え直す。待ちが済んだら --end で消す。DONE は、渡したファイルのうち終わった
//! ものの 1 つを出す。
//!
//! 出力と終了コード:
//! - `DONE: <ファイル>` 0: 渡した background の出力のどれかが終わった（Claude Code が最後の行に `[exited with code N]` か
//!   `[killed]` を書く）
//! - `WAIT: 途中の報告から <経過> 分` 2: 1 歩の時間がたった
//! - `REPORT: 途中の報告から <経過> 分を超えた…` 3: 途中の報告から 55 分を超えた（数え直した）
//! - 使い方の誤り、渡した出力のファイルが無い（パスのコピーミス）、状態を書けないときは、理由を出して 1
//!
//! DONE、WAIT、REPORT の行の後には、担当の次の行動を `NEXT:` の行で出す（DONE は、残りのファイルで呼び直すコマンドと
//! --end のコマンドを、引数を埋めて出す）。
//!
//! テストは WAIT_STEP_SECONDS（1 歩、既定 270）と WAIT_STEP_REPORT（途中の報告の間隔の秒、既定 3300）で時間を縮める

use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const USAGE: &str = "Usage: wait-step.sh <自分の worktree のパス> [<background の出力のファイル>...] | --end <自分の worktree のパス>";

/// 担当が貼る形のコマンドの頭。スキルの allowed-tools は `~` のパスで許すので、展開したパスにしない
const SCRIPT: &str = "~/.claude/scripts/wait-step.sh";

pub fn main(args: &[String]) -> i32 {
    match args {
        [end, key] if end == "--end" && !key.is_empty() => {
            // 無い状態を消すのは成功（sh の rm -f と同じ）
            let _ = std::fs::remove_file(state_of(key));
            0
        }
        [end, ..] if end == "--end" => usage(),
        [key, files @ ..] if !key.is_empty() => wait(key, files),
        _ => usage(),
    }
}

fn usage() -> i32 {
    eprintln!("{USAGE}");
    1
}

fn env_secs(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 鍵のパスを正規化する（相対を絶対に、`.` と `..` と末尾の `/` を揃える。sh の `realpath -m` と同じく、無いパスも受ける）
fn normalize(key: &str) -> PathBuf {
    let path = Path::new(key);
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// 状態のファイル。鍵は正規化したパスのハッシュにしてファイル名に使う（パスの記号や日本語でも重ならない）。ハッシュは
/// 版によらず同じ値になる FNV-1a（std の DefaultHasher は版で変わりうる）
fn state_of(key: &str) -> PathBuf {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in normalize(key).as_os_str().as_encoded_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    std::env::temp_dir()
        .join("claude-wait-step")
        .join(format!("{h:016x}"))
}

/// 状態（途中の報告の時刻, 最後に呼んだ時刻）。無いか読めなければ None
fn read_state(path: &Path) -> Option<(u64, u64)> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut it = text.split_whitespace().map(|v| v.parse::<u64>().ok());
    Some((it.next()??, it.next()??))
}

fn save(path: &Path, reported: u64) -> Result<(), i32> {
    let write = || -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, format!("{reported} {}\n", now()))
    };
    write().map_err(|_| {
        eprintln!("ERROR: 待ちの状態を書けません: {}", path.display());
        1
    })
}

/// 最後の行が終わりの印か。最後の行だけを見る（途中の行の同じ文字列で誤らず、大きな出力を毎秒読み直さない）
fn finished(path: &str) -> bool {
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(4096);
    if f.seek(SeekFrom::Start(start)).is_err() {
        return false;
    }
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return false;
    }
    let text = String::from_utf8_lossy(&buf);
    let last = text
        .strip_suffix('\n')
        .unwrap_or(&text)
        .rsplit('\n')
        .next()
        .unwrap_or("");
    let last = last.strip_suffix('\r').unwrap_or(last);
    last.starts_with("[exited with code") || last.starts_with("[killed")
}

/// bash に貼れる形の 1 語。記号の無い語（日本語を含む）はそのまま、ほかは単一引用符で囲む
fn quote(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| !c.is_ascii() || c.is_ascii_alphanumeric() || "_-./:@%+=,".contains(c));
    if plain {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

fn command(args: &[&str]) -> String {
    std::iter::once(SCRIPT.to_string())
        .chain(args.iter().map(|a| quote(a)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn print_done(key: &str, files: &[String], done: &str) {
    println!("DONE: {done}");
    let rest: Vec<&str> = files
        .iter()
        .map(String::as_str)
        .filter(|f| *f != done)
        .collect();
    let end = command(&["--end", key]);
    if rest.is_empty() {
        println!(
            "NEXT: このファイルの結果を取り込む。ほかにも出力のファイルの無い待ち（/code-review、親の返事）があれば、ターンを終えずに {} で呼び直し、すべて済んだら {end} で待ちを消す",
            command(&[key])
        );
    } else {
        let again: Vec<&str> = std::iter::once(key).chain(rest).collect();
        println!(
            "NEXT: このファイルの結果を取り込む。ほかにも待つもの（残りの出力のファイル、出力のファイルの無い /code-review、親の返事）があれば、ターンを終えずに {} で呼び直し、すべて済んだら {end} で待ちを消す",
            command(&again)
        );
    }
}

fn wait(key: &str, files: &[String]) -> i32 {
    if let Some(f) = files.iter().find(|f| !Path::new(f).is_file()) {
        eprintln!("ERROR: 出力のファイルがありません: {f}");
        return 1;
    }
    let state = state_of(key);
    let step = env_secs("WAIT_STEP_SECONDS", 270);
    let report = env_secs("WAIT_STEP_REPORT", 3300);
    let start = now();
    // 前の呼び出しから報告の間隔を超えて空いていれば、終わった待ちの時計なので数え直す
    let mut reported = match read_state(&state) {
        Some((r, last)) if start.saturating_sub(last) <= report => r,
        _ => start,
    };
    if let Err(code) = save(&state, reported) {
        return code;
    }
    // 1 秒ごとに、終わり、途中の報告の時刻、1 歩の時刻の順に見る（終わったものに途中の報告を出さない）。返る前に最後に
    // 呼んだ時刻を残す
    let deadline = start + step;
    loop {
        if let Some(done) = files.iter().find(|f| finished(f)) {
            if let Err(code) = save(&state, reported) {
                return code;
            }
            print_done(key, files, done);
            return 0;
        }
        let t = now();
        if t.saturating_sub(reported) >= report {
            let elapsed = (t - reported) / 60;
            reported = t;
            if let Err(code) = save(&state, reported) {
                return code;
            }
            println!(
                "REPORT: 途中の報告から {elapsed} 分を超えた。親に途中の報告を送ってから、もう一度呼ぶ"
            );
            println!(
                "NEXT: SendMessage（to: \"main\"）で「<待つもの> の待ちが 55 分を超えた。終わり次第、残りの作業を続ける」とだけ送り、待つのも作業もやめずに、ターンを終えずに同じ引数でもう一度呼ぶ。<待つもの> には、止まって通知の来ない /code-review と返事の来ない許可も入れる（止めるかは親の返事で決める）"
            );
            return 3;
        }
        if t >= deadline {
            if let Err(code) = save(&state, reported) {
                return code;
            }
            println!("WAIT: 途中の報告から {} 分", (t - reported) / 60);
            println!(
                "NEXT: ターンを終えずに、同じ引数でもう一度呼ぶ。呼び出しの後に通知（終わりの通知、親の返事）が届いていれば、それで続ける。途中で親に報告するのは REPORT の後だけ"
            );
            return 2;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}
