//! `wait-step.sh <自分の worktree のパス> [<background の出力のファイル>...]` / `wait-step.sh --end <自分の worktree のパス>`:
//! /delegate の担当が、ターンを終えずに待つ 1 ステップ（ADR-0082。`~/.claude/scripts/wait-step.sh` が呼ぶ）。
//!
//! 担当は、待つ間これをフォアグラウンドで続けて呼ぶ。ターンを終えると、そのたびに親に通知が届き親が応答するので、終えずに
//! 待つ。1 回の呼び出しは約 4 分 30 秒で返るので、呼ぶたびの応答で担当のキャッシュ（5 分で切れる）が保たれる。待つ間に届いた
//! 通知（/code-review の終わり、親の返事など）は、呼び出しが返った後に担当に渡る。
//!
//! 時計は担当ごとに 1 つで、worktree のパスで分ける（並行する担当どうしで、リポジトリやブランチの名前が重なっても取り合わ
//! ない）。親に最後に送った時刻（初めは待ち始め）と、最後に呼んだ時刻を `<一時のディレクトリ>/claude-wait-step/<鍵のハッシュ>` に
//! 残す。親に最後に送ってから線（55 分。[`delegate_wait::LIMIT_SECS`]。投げる前の見込みの線と同じ）を超えたら END を出し、
//! 時計を消す（担当は待ちをやめ、投げたジョブは止めずに、残りの作業を報告に書いて終える。線は親のキャッシュの 1 時間が切れる
//! 前に終えるためのもの。ADR-0078 の 2026-11-20 の変更）。親に送った時刻は、担当の SendMessage と SubagentHandback の前に hook の
//! restart-wait-clock が [`restart_from_contact`] で書く（担当の判断に任せない）。前の呼び出しから線を超えて空いていれば、
//! 終わった待ち（--end の忘れ、中断した担当）の時計なので数え直す。待ちが済んだら --end で消す。DONE は、渡したファイルのうち
//! 終わったものの 1 つを出す。
//!
//! 出力と終了コード:
//! - `DONE: <ファイル>` 0: 渡した background の出力のどれかが終わった（Claude Code が最後の行に `[exited with code N]` か
//!   `[killed]` を書く）
//! - `WAIT: 親に最後に送ってから <経過> 分` 2: 1 ステップの時間がたった
//! - `END: 親に最後に送ってから <経過> 分を超えた…` 3: 親に最後に送ってから線を超えた
//! - 使い方の誤り、渡した出力のファイルが無い（パスのコピーミス）、状態を書けないときは、理由を出して 1
//!
//! DONE、WAIT、END の行の後には、担当の次の行動を `NEXT:` の行で出す（DONE は、残りのファイルで呼び直すコマンドと
//! --end のコマンドを、引数を埋めて出す）。
//!
//! テストは WAIT_STEP_SECONDS（1 ステップ、既定 270）と WAIT_STEP_LIMIT（線の秒、既定 3300）で時間を縮め、
//! WAIT_STEP_NOW（UNIX の秒）で時計を差し替える。どれも 0 以上の整数でなければ、理由を出して 1

use super::delegate_wait;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const USAGE: &str = "Usage: wait-step.sh <自分の worktree のパス> [<background の出力のファイル>...] | --end <自分の worktree のパス>";

/// 担当が貼る形のコマンドの頭。スキルの allowed-tools は `~` のパスで許すので、展開したパスにしない
const SCRIPT: &str = "~/.claude/scripts/wait-step.sh";

pub fn main(args: &[String]) -> i32 {
    match args {
        [end, key] if end == "--end" && !key.is_empty() => {
            // 無い状態を消すのも成功（待ちを消し忘れたかを担当に問わない）
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

/// 秒の環境変数。無ければ None、読めなければ理由を出して Err（黙って壁の時計や既定の秒数に替えると、テストが
/// 気づかずに壁の時計に頼る）
fn env_secs(name: &str) -> Result<Option<u64>, ()> {
    match std::env::var(name) {
        Ok(v) => v.parse().map(Some).map_err(|_| {
            eprintln!("ERROR: {name} は 0 以上の整数の秒にしてください: {v}");
        }),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(v)) => {
            eprintln!(
                "ERROR: {name} は 0 以上の整数の秒にしてください: {}",
                v.to_string_lossy()
            );
            Err(())
        }
    }
}

/// 1 回の呼び出しの時計。WAIT_STEP_NOW があれば、その時刻から始め、眠らずに 1 秒の見回りごとに 1 秒進める。壁の
/// 時計は秒に切り捨てて約 1 秒ごとに読むので、起動の遅れや眠りの延びが秒の境目をまたぐと、秒の差の小さい 1 ステップと
/// 途中の報告の順が入れ替わる。テストはこの時計で時刻の並びを決める
struct Clock {
    fake: Option<u64>,
}

impl Clock {
    fn now(&self) -> u64 {
        self.fake.unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        })
    }

    fn sleep(&mut self) {
        match &mut self.fake {
            Some(t) => *t = t.saturating_add(1),
            None => std::thread::sleep(Duration::from_secs(1)),
        }
    }
}

/// 鍵のパスを正規化する。相対を絶対にし、`.` と `..` と末尾の `/` を揃え、在る部分の symlink を解く（同じ worktree を
/// symlink を通ったパスで渡しても、同じ時計にする）。無い部分はそのまま後ろに付ける
fn normalize(key: &str) -> PathBuf {
    let path = Path::new(key);
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut lexical = PathBuf::new();
    for c in abs.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                lexical.pop();
            }
            other => lexical.push(other.as_os_str()),
        }
    }
    let mut existing = lexical.as_path();
    loop {
        if let Ok(real) = std::fs::canonicalize(existing) {
            let rest = lexical.strip_prefix(existing).unwrap_or(Path::new(""));
            return if rest.as_os_str().is_empty() {
                real
            } else {
                real.join(rest)
            };
        }
        match existing.parent() {
            Some(parent) => existing = parent,
            None => return lexical,
        }
    }
}

/// 状態のファイル。鍵は正規化したパスのハッシュにしてファイル名に使う（パスの記号や日本語でも重ならない）。ハッシュは
/// 版によらず同じ値になる FNV-1a（std の DefaultHasher は版で変わりうる）
fn state_of(key: &str) -> PathBuf {
    let h = fnv1a(normalize(key).as_os_str().as_encoded_bytes());
    std::env::temp_dir()
        .join("claude-wait-step")
        .join(format!("{h:016x}"))
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// 担当（hook の入力の agent_id）が最後に呼んだ wait-step の鍵を残すファイル。hook の restart-wait-clock が、担当が親に
/// 送ったときに、その鍵の時計を数え直すのに使う
fn agent_file(agent_id: &str) -> PathBuf {
    std::env::temp_dir()
        .join("claude-wait-step")
        .join("agents")
        .join(format!("{:016x}", fnv1a(agent_id.as_bytes())))
}

/// 担当 agent_id が鍵 key で待っていることを残す（hook が wait-step.sh の呼び出しの前に呼ぶ。失敗は無視する: 残せなければ
/// 数え直さず、線が早めに来るだけ）
pub(crate) fn note_waiter(agent_id: &str, key: &str) {
    let path = agent_file(agent_id);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, key);
}

/// 担当 agent_id が親に送った時刻 now から、その担当の待ちの時計を数え直す（hook が SendMessage と SubagentHandback の
/// 前に呼ぶ）。待っていなければ何もしない
pub(crate) fn restart_from_contact(agent_id: &str, now: u64) {
    let Ok(key) = std::fs::read_to_string(agent_file(agent_id)) else {
        return;
    };
    let state = state_of(&key);
    if let Some((_, called)) = read_state(&state) {
        let _ = save(&state, now, called);
    }
}

/// 状態（親に最後に送った時刻（初めは待ち始め）, 最後に呼んだ時刻）。無いか読めなければ None
fn read_state(path: &Path) -> Option<(u64, u64)> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut it = text.split_whitespace().map(|v| v.parse::<u64>().ok());
    Some((it.next()??, it.next()??))
}

/// 状態に、親に最後に送った時刻と、最後に呼んだ時刻 called を残す
fn save(path: &Path, started: u64, called: u64) -> Result<(), i32> {
    let write = || -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, format!("{started} {called}\n"))
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

/// bash に貼れる形の 1 語。記号の無い語（日本語を含む）はそのまま、ほかは単一引用符で囲む（base-moved、review-level、
/// worktree-add の案内のコマンドも使う）
pub(crate) fn quote(word: &str) -> String {
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
    let (step, limit, fake) = match (
        env_secs("WAIT_STEP_SECONDS"),
        env_secs("WAIT_STEP_LIMIT"),
        env_secs("WAIT_STEP_NOW"),
    ) {
        (Ok(step), Ok(limit), Ok(fake)) => (
            step.unwrap_or(270),
            limit.unwrap_or(delegate_wait::LIMIT_SECS),
            fake,
        ),
        _ => return 1,
    };
    let mut clock = Clock { fake };
    let start = clock.now();
    // 前の呼び出しから線を超えて空いていれば、終わった待ちの時計なので数え直す
    let started = match read_state(&state) {
        // 親に送った時刻が今より後なら、時計が戻ったので数え直す（戻った分だけ END が遅れないように）
        Some((s, last)) if start.saturating_sub(last) <= limit && s <= start => s,
        _ => start,
    };
    if let Err(code) = save(&state, started, start) {
        return code;
    }
    // 1 秒ごとに、終わり、線、1 ステップの時刻の順に見る（終わったものに待ちの終わりを出さない）。返る前に最後に
    // 呼んだ時刻を残す
    let deadline = start.saturating_add(step);
    loop {
        let t = clock.now();
        if let Some(done) = files.iter().find(|f| finished(f)) {
            if let Err(code) = save(&state, started, t) {
                return code;
            }
            print_done(key, files, done);
            return 0;
        }
        let elapsed = t.saturating_sub(started) / 60;
        if t.saturating_sub(started) >= limit {
            // 終える担当の時計は消す（--end を忘れても、同じ worktree の次の担当が古い時刻を受け継がない）
            let _ = std::fs::remove_file(&state);
            println!(
                "END: 親に最後に送ってから {elapsed} 分を超えた（線は {} 分）。待ちをやめて、報告して終える",
                limit / 60
            );
            println!(
                "NEXT: 待つのをやめる。投げたジョブと、その background の slurm-run.sh は止めない（scancel も TaskStop もしない。親が slurm-wait.sh で番号から引き継ぐ）。待っていたもの（ジョブの番号と background の出力のファイル、レビュー、返事の来ない許可）と残りの作業を報告の「続きの作業」に書き、{} で待ちを消してから、/delegate の手順 9 のとおり SubagentHandback で報告して終える",
                command(&["--end", key])
            );
            return 3;
        }
        if t >= deadline {
            if let Err(code) = save(&state, started, t) {
                return code;
            }
            println!("WAIT: 親に最後に送ってから {elapsed} 分");
            println!(
                "NEXT: ターンを終えずに、同じ引数でもう一度呼ぶ。呼び出しの後に通知（終わりの通知、親の返事）が届いていれば、それで続け、待つものがすべて済んだら {} で待ちを消す。線を超えると END を出す",
                command(&["--end", key])
            );
            return 2;
        }
        clock.sleep();
    }
}
