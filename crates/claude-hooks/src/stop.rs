//! `claude-hooks stop [claude-dir]`: Claude Code の Stop の hook（応答の終わりごと）。自動メモリ
//! （`<claude-dir>/projects/*/memory/` の下）を、`<claude-dir>` の git の ref `refs/memory-backup` にコミットする
//! （ADR-0076、ADR-0089）。refs/heads の下に置かないのは、ブランチとして checkout すると、main で追跡しているファイル
//! （settings.json など）が作業ツリーから消えるため。中身が前のコミットと同じならコミットしない。
//!
//! claude-dir を省くと `~/.claude`。標準入力に hook の JSON があれば、`session_id` と `cwd` をコミットの本文に入れる
//! （端末なら読まない。読めない JSON と、文字列でない値は `-` にする。本文の飾りのために退避を止めない）。
//!
//! 戻し方の例:
//!   git -C ~/.claude log --stat refs/memory-backup -- projects/<プロジェクト>/memory/<ファイル>
//!   git -C ~/.claude show refs/memory-backup~3:projects/<プロジェクト>/memory/<ファイル>
//!
//! 退避の後に、hook の JSON の `last_assistant_message`（最後の応答の本文）が英語で書かれていれば、1 行目に理由を、
//! 続けて日本語で書き直す修正方法を書いて終了コード 2 で終わる（ADR-0108。判定は src/response_language.rs）。2 は応答を
//! 続けさせ、標準エラーが Claude に渡る。JSON が読めない、本文が無い、`stop_hook_active` が true のときは見ない。
//! 登録を別にしないのは、settings.json と claude-hooks の版がずれたとき、前の版が知らないサブコマンドに 2 で終え、
//! 応答の終わりごとに続けさせるため
//!
//! 退避に失敗したときは、1 行目に理由を書いて終了コード 1 で終わる。Claude Code は Stop の hook の 0 と 2 のほかの
//! 終了コードを止めない失敗として扱い、標準エラーの 1 行目を通知に出す。引数の誤りでも 2 は使わない

use std::ffi::{OsStr, OsString};
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const REF: &str = "refs/memory-backup";
/// ほかの退避がロックを放すまで待つ長さ。settings.json の Stop の timeout（claude-setup の STOP_TIMEOUT）より短くし、
/// 待ちきれないときに hook が殺されず、理由を出して終わるようにする
const LOCK_WAIT: Duration = Duration::from_secs(30);
const LOCK_POLL: Duration = Duration::from_millis(50);

pub fn main(args: &[String]) -> i32 {
    let dir = match args {
        [] => crate::shell::home().join(".claude"),
        [dir] => PathBuf::from(dir),
        _ => {
            eprintln!("claude-hooks stop: 引数が多すぎます");
            eprintln!("Usage: claude-hooks stop [claude-dir]");
            return 1;
        }
    };
    let input = read_input();
    let backup = backup(&dir, &input);
    let english = english_response(&input);
    if let Some(counts) = &english {
        eprintln!(
            "claude-hooks stop: 最後の応答が日本語で書かれていません（英語の語 {}、かなと漢字 {} 字）",
            counts.english_words, counts.japanese_chars
        );
        eprintln!(
            "ユーザーへの応答は日本語で書く指示があります（settings の language と、システムプロンプトの Language の節）。\
             同じ内容を日本語で書き直して、応答し直してください。コード、パス、識別子、引用は元のままでかまいません"
        );
        eprintln!(
            "ユーザーかスキルが英語の本文を求めた応答（英語のメール、PR の本文、翻訳など）なら、本文は訳さず、             コードブロックか引用に入れ、日本語の説明を添えて応答し直してください"
        );
        eprintln!(
            "（英語の語が {} 以上で、かなと漢字が {} 字未満の応答を英語と見ています。コードブロック、インラインのコード、\
             引用のブロックは数えていません）",
            crate::response_language::MIN_ENGLISH_WORDS,
            crate::response_language::MIN_JAPANESE_CHARS
        );
    }
    if let Err(reason) = &backup {
        eprintln!(
            "claude-hooks stop: メモリを {} の {REF} にコミットできませんでした（この応答の版は残っていません）",
            dir.display()
        );
        eprintln!("{reason}");
    }
    // 2 では標準エラーが Claude に渡り、利用者の通知には出ない。退避の失敗が重なっても、書き直した応答の Stop で退避を
    // やり直し、失敗すれば 1 で通知に出る
    match (english, backup) {
        (Some(_), _) => 2,
        (None, Ok(())) => 0,
        (None, Err(_)) => 1,
    }
}

/// 最後の応答が英語なら、その数え方の結果。Stop の hook が応答を続けさせた後（stop_hook_active）は見ない。書き直しても
/// 英語のままなら、続けて止めても直る見込みが小さく、手戻りだけが増える
fn english_response(input: &Input) -> Option<crate::response_language::Counts> {
    if input.stop_hook_active {
        return None;
    }
    let counts = crate::response_language::count(input.message.as_deref()?);
    counts.is_english().then_some(counts)
}

/// hook の JSON の session_id、cwd、stop_hook_active、last_assistant_message
struct Input {
    session: String,
    cwd: String,
    stop_hook_active: bool,
    message: Option<String>,
}

fn read_input() -> Input {
    let mut text = String::new();
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        let _ = stdin.lock().read_to_string(&mut text);
    }
    let value: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    let field = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or("-")
            .to_string()
    };
    Input {
        session: field("session_id"),
        cwd: field("cwd"),
        stop_hook_active: value
            .get("stop_hook_active")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        message: value
            .get("last_assistant_message")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    }
}

/// claude-dir で動かす git。hook は親の環境を継ぐので、GIT_DIR などを export したシェル（git の hook の中、dotfiles の
/// 別名）から起動した Claude Code では、外す前の git が claude-dir でなくそのリポジトリと index を相手にする
struct Git {
    dir: PathBuf,
    local_env: Vec<OsString>,
    index: Option<PathBuf>,
}

impl Git {
    fn new(dir: &Path) -> Result<Git, String> {
        if !dir.is_dir() {
            return Err(format!("{} はディレクトリではありません", dir.display()));
        }
        let mut git = Git {
            dir: dir.to_path_buf(),
            local_env: Vec::new(),
            index: None,
        };
        // 一覧はリポジトリの外でも出る（git の版の持つ変数の一覧）
        git.local_env = git
            .text(&["rev-parse", "--local-env-vars"])?
            .lines()
            .map(OsString::from)
            .collect();
        Ok(git)
    }

    fn output<S: AsRef<OsStr>>(&self, args: &[S], input: Option<&[u8]>) -> Result<Output, String> {
        let mut cmd = Command::new("git");
        for var in &self.local_env {
            cmd.env_remove(var);
        }
        if let Some(index) = &self.index {
            cmd.env("GIT_INDEX_FILE", index);
        }
        cmd.args(args)
            .current_dir(&self.dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let out = match input {
            Some(input) => {
                crate::process::output_with_input(&mut cmd, input).map_err(|e| match e {
                    crate::process::Unfinished::NotStarted(e)
                    | crate::process::Unfinished::Lost(e) => e,
                })
            }
            None => cmd.stdin(Stdio::null()).output(),
        }
        .map_err(|e| format!("git {} を実行できません（{e}）", show(args)))?;
        Ok(out)
    }

    /// 成功したときの標準出力（末尾の改行を除く）。失敗は git の標準エラーを理由にする
    fn run<S: AsRef<OsStr>>(&self, args: &[S], input: Option<&[u8]>) -> Result<String, String> {
        let out = self.output(args, input)?;
        if !out.status.success() {
            return Err(format!(
                "git {} が失敗しました: {}",
                show(args),
                String::from_utf8_lossy(&out.stderr).trim_end()
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout)
            .trim_end_matches(['\n', '\r'])
            .to_string())
    }

    fn text(&self, args: &[&str]) -> Result<String, String> {
        self.run(args, None)
    }
}

/// 失敗の文に出す git の引数
fn show<S: AsRef<OsStr>>(args: &[S]) -> String {
    let args: Vec<_> = args.iter().map(|a| a.as_ref().to_string_lossy()).collect();
    args.join(" ")
}

fn backup(dir: &Path, input: &Input) -> Result<(), String> {
    let mut git = Git::new(dir)?;
    // 上の階層のリポジトリ（ホームを丸ごと追跡するものなど）に、知らずに ref を作らない。直下かは prefix の空で見る
    // （パスどうしを比べると、Windows で git の `C:/…` と OS の `C:\…` の書き方が食い違う）
    if !git.text(&["rev-parse", "--show-prefix"])?.is_empty() {
        let top = git.text(&["rev-parse", "--show-toplevel"])?;
        return Err(format!(
            "{} はリポジトリの直下ではありません（直下: {top}）",
            dir.display()
        ));
    }
    let gitdir = PathBuf::from(git.text(&["rev-parse", "--absolute-git-dir"])?);

    // 同時に終わった応答の hook どうしで、同じ親から 2 つのコミットを作って片方の変更を失わないよう、親を読んでから
    // ref を進めるまでを 1 つずつにする。std の File は子に継がれない（Unix は O_CLOEXEC、Windows は継承しない
    // ハンドル）ので、git が起動して残る子（fsmonitor の daemon、detach する保守）がロックを持ち続けることはない
    let lock_path = gitdir.join("memory-backup.lock");
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .map_err(|e| format!("ロック {} を開けません（{e}）", lock_path.display()))?;
    let start = Instant::now();
    loop {
        match lock.try_lock() {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock) if start.elapsed() < LOCK_WAIT => {
                std::thread::sleep(LOCK_POLL)
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(format!(
                    "ロック {} を {} 秒で取れませんでした（ほかの退避が終わっていません）",
                    lock_path.display(),
                    LOCK_WAIT.as_secs()
                ));
            }
            Err(std::fs::TryLockError::Error(e)) => {
                return Err(format!(
                    "ロック {} を取れません（{e}）",
                    lock_path.display()
                ));
            }
        }
    }

    // 作業ツリーの index でなく一時の index に足す。利用者がステージした変更と、main の HEAD に触らない。index は
    // メモリの中身を指すので、持ち主だけが読める場所に置く
    let tmp = crate::process::private_temp_dir("claude-hooks-memory-backup")?;
    git.index = Some(tmp.0.join("index"));

    let mems = memory_dirs(dir)?;
    if !mems.is_empty() {
        // -f: ~/.claude の .gitignore（手で管理していて、ホストごとに無いこともある）に頼らず、メモリだけを入れる
        let mut args: Vec<&OsStr> = ["add", "-f", "--"].map(OsStr::new).to_vec();
        args.extend(mems.iter().map(|m| m.as_os_str()));
        git.run(&args, None)?;
    }
    let tree = git.text(&["write-tree"])?;
    let parent = git.output(
        &["rev-parse", "-q", "--verify", &format!("{REF}^{{commit}}")],
        None,
    )?;
    let parent = parent.status.success().then(|| {
        String::from_utf8_lossy(&parent.stdout)
            .trim_end()
            .to_string()
    });
    match &parent {
        None if mems.is_empty() => return Ok(()),
        Some(p) if git.text(&["rev-parse", &format!("{p}^{{tree}}")])? == tree => return Ok(()),
        _ => {}
    }

    // git commit でなく commit-tree と update-ref で refs/memory-backup だけを進める。git commit は HEAD のブランチ
    // （~/.claude では main）にコミットし、main への直接のコミットを止める pre-commit の hook と、メッセージと中身の
    // 検査（commit-msg、機密情報、日本語の語）に掛かる。バックアップは壊れた中身もそのまま残すのが目的で、検査で
    // 止まると、その間の版が残らない
    let message = format!(
        "memory: snapshot\n\nsession: {}\ncwd: {}\n",
        input.session, input.cwd
    );
    let mut args = vec!["commit-tree", tree.as_str()];
    if let Some(p) = &parent {
        args.extend(["-p", p.as_str()]);
    }
    args.extend(["-F", "-"]);
    let commit = git.run(&args, Some(message.as_bytes()))?;
    // 古い値に親（無ければ空 = まだ無いこと）を渡し、ロックの外で動かされていたら失敗させる
    git.text(&[
        "update-ref",
        "-m",
        "claude-memory-backup",
        REF,
        &commit,
        parent.as_deref().unwrap_or(""),
    ])?;
    drop(lock);
    Ok(())
}

/// `projects/*/memory`（claude-dir からの相対パス）。`.` で始まる名前は読まない。symlink の
/// メモリのディレクトリは、git add がリンク（mode 120000）だけを入れて中身を退避しないので、成功と見せずに止める。
/// リンクを追うと、リポジトリの外のファイルまで取り込める
fn memory_dirs(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let projects = dir.join("projects");
    let entries = match std::fs::read_dir(&projects) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{} を読めません（{e}）", projects.display())),
    };
    let mut mems = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("{} を読めません（{e}）", projects.display()))?;
        let name = entry.file_name();
        if name.as_encoded_bytes().starts_with(b".") || !entry.path().is_dir() {
            continue;
        }
        let rel = Path::new("projects").join(&name).join("memory");
        let path = dir.join(&rel);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(format!(
                    "{} は symlink で、中身を退避できません",
                    path.display()
                ));
            }
            Ok(_) => mems.push(rel),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{} を読めません（{e}）", path.display())),
        }
    }
    mems.sort();
    Ok(mems)
}
