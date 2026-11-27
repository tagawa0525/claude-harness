//! `claude-hooks stop [claude-dir]`（Claude Code の Stop の hook）が、自動メモリ（`<claude-dir>/projects/*/memory/`）だけを
//! `<claude-dir>` の git の ref `refs/memory-backup` にコミットし、英語で書いた最後の応答を 2 で返すことを確かめる
//! （ADR-0076、ADR-0089、ADR-0108）

mod common;

use common::forget_git_env;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

const REF: &str = "refs/memory-backup";

/// テストごとの一時の場所。`home/` を HOME に、`gitconfig` をグローバルの設定にする
struct Work {
    root: PathBuf,
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Work {
    fn new() -> Work {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "stop-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("home")).unwrap();
        std::fs::create_dir_all(root.join("tmp")).unwrap();
        let work = Work { root };
        std::fs::write(
            work.root.join("gitconfig"),
            "[user]\n\tname = test\n\temail = test@example.com\n[init]\n\tdefaultBranch = main\n",
        )
        .unwrap();
        work
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    fn env(&self, cmd: &mut Command) {
        forget_git_env(cmd)
            .env("HOME", self.path("home"))
            .env("GIT_CONFIG_GLOBAL", self.path("gitconfig"))
            // 一時の index の置き場（std::env::temp_dir）。Unix は TMPDIR、Windows は TMP を読む
            .env("TMPDIR", self.path("tmp"))
            .env("TMP", self.path("tmp"))
            .env("TEMP", self.path("tmp"))
            // 一時の場所は target の下（手元では worktree の中）にあるので、外のリポジトリを見つけさせない
            .env("GIT_CEILING_DIRECTORIES", &self.root)
            .env_remove("CLAUDECODE");
    }

    /// テストの準備と確かめに使う git（失敗したら止める）
    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.git_output(dir, args);
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }

    fn git_output(&self, dir: &Path, args: &[&str]) -> Output {
        let mut cmd = Command::new("git");
        self.env(&mut cmd);
        cmd.args(args).current_dir(dir).output().unwrap()
    }

    /// ref が指すコミット（無ければ空）
    fn backup(&self, dir: &Path) -> String {
        let out = self.git_output(dir, &["rev-parse", "-q", "--verify", REF]);
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }

    /// `git init` したリポジトリ
    fn repo(&self, rel: &str) -> PathBuf {
        let dir = self.path(rel);
        std::fs::create_dir_all(&dir).unwrap();
        self.git(&dir, &["init", "-q"]);
        dir
    }

    /// hook と同じく標準入力に JSON を渡して呼ぶ
    fn stop(&self, args: &[&Path], input: &str) -> Output {
        self.stop_with(args, input, |_| {})
    }

    fn stop_with(&self, args: &[&Path], input: &str, f: impl FnOnce(&mut Command)) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_claude-hooks"));
        self.env(&mut cmd);
        cmd.arg("stop")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        f(&mut cmd);
        let mut child = cmd.spawn().unwrap();
        use std::io::Write;
        // 引数の誤りの子は読まずに終わるので、書けない（EPIPE）ことは失敗にしない
        let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
        child.wait_with_output().unwrap()
    }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn assert_ok(out: &Output) {
    assert!(out.status.success(), "失敗した: {}", stderr(out));
}

/// 失敗は、1 行目に理由を書いて 1 で終える（Claude Code は Stop の hook の 0 と 2 のほかを止めない失敗として扱い、
/// 標準エラーの 1 行目を通知に出す。2 は応答を続けさせる）
fn assert_failed(out: &Output) -> String {
    assert_eq!(out.status.code(), Some(1), "{}", stderr(out));
    let err = stderr(out);
    assert!(
        err.lines()
            .next()
            .unwrap_or_default()
            .starts_with("claude-hooks stop: "),
        "1 行目が名前で始まらない: {err}"
    );
    err
}

/// `~/.claude` に見立てたリポジトリ（main に settings.json をコミットし、変更をステージしたもの）
fn claude_dir(work: &Work) -> PathBuf {
    let cd = work.repo("claude");
    write(&cd.join("settings.json"), "settings\n");
    work.git(&cd, &["add", "settings.json"]);
    work.git(&cd, &["commit", "-q", "-m", "init"]);
    write(&cd.join("projects/-a/memory/MEMORY.md"), "index a\n");
    write(&cd.join("projects/-b/memory/note.md"), "note b\n");
    write(&cd.join("projects/-a/session.jsonl"), "log\n");
    write(&cd.join("projects/-a/sessions/s.json"), "x\n");
    write(&cd.join("other.txt"), "top\n");
    write(&cd.join("settings.json"), "settings2\n");
    work.git(&cd, &["add", "settings.json"]);
    cd
}

#[test]
fn commits_only_the_memory_files_and_leaves_head_index_and_branches() {
    let work = Work::new();
    let cd = claude_dir(&work);
    let head = work.git(&cd, &["rev-parse", "HEAD"]);
    let staged = work.git(&cd, &["diff", "--cached", "--name-status"]);

    assert_ok(&work.stop(&[&cd], r#"{"session_id":"s1","cwd":"/w"}"#));

    assert_eq!(
        work.git(&cd, &["ls-tree", "-r", "--name-only", REF]),
        "projects/-a/memory/MEMORY.md\nprojects/-b/memory/note.md"
    );
    assert_eq!(work.git(&cd, &["rev-list", "--count", REF]), "1");
    assert_eq!(
        work.git(&cd, &["log", "-1", "--format=%B", REF]),
        "memory: snapshot\n\nsession: s1\ncwd: /w"
    );
    assert_eq!(work.git(&cd, &["rev-parse", "HEAD"]), head);
    assert_eq!(work.git(&cd, &["branch", "--show-current"]), "main");
    assert_eq!(
        work.git(
            &cd,
            &["for-each-ref", "--format=%(refname:short)", "refs/heads"]
        ),
        "main"
    );
    assert_eq!(
        work.git(&cd, &["diff", "--cached", "--name-status"]),
        staged
    );
}

#[test]
fn adds_a_commit_only_when_the_memory_changes() {
    let work = Work::new();
    let cd = claude_dir(&work);
    assert_ok(&work.stop(&[&cd], "{}"));
    let first = work.backup(&cd);

    assert_ok(&work.stop(&[&cd], "{}"));
    assert_eq!(work.backup(&cd), first, "中身が同じならコミットを足さない");

    write(&cd.join("projects/-a/memory/MEMORY.md"), "index a2\n");
    assert_ok(&work.stop(&[&cd], "{}"));
    assert_eq!(work.git(&cd, &["rev-parse", &format!("{REF}^")]), first);
    assert_eq!(
        work.git(
            &cd,
            &["show", &format!("{REF}:projects/-a/memory/MEMORY.md")]
        ),
        "index a2"
    );

    std::fs::remove_file(cd.join("projects/-b/memory/note.md")).unwrap();
    assert_ok(&work.stop(&[&cd], "{}"));
    assert_eq!(
        work.git(&cd, &["ls-tree", "-r", "--name-only", REF]),
        "projects/-a/memory/MEMORY.md"
    );
    assert_eq!(
        work.git(
            &cd,
            &["show", &format!("{REF}~1:projects/-b/memory/note.md")]
        ),
        "note b"
    );
}

#[test]
fn reads_a_missing_or_broken_hook_input_as_unknown_session() {
    let work = Work::new();
    let cd = claude_dir(&work);
    assert_ok(&work.stop(&[&cd], "not json"));
    assert_eq!(
        work.git(&cd, &["log", "-1", "--format=%B", REF]),
        "memory: snapshot\n\nsession: -\ncwd: -"
    );

    write(&cd.join("projects/-a/memory/MEMORY.md"), "index a2\n");
    assert_ok(&work.stop(&[&cd], r#"{"session_id":null,"cwd":3}"#));
    assert_eq!(
        work.git(&cd, &["log", "-1", "--format=%B", REF]),
        "memory: snapshot\n\nsession: -\ncwd: -"
    );
}

/// グローバルに配る pre-commit、commit-msg などの hook と、main への直接のコミットを止める検査に掛からない
#[cfg(unix)]
#[test]
fn commits_under_failing_git_hooks_and_claudecode() {
    let work = Work::new();
    let cd = claude_dir(&work);
    let hooks = work.path("hooks");
    for h in [
        "pre-commit",
        "commit-msg",
        "prepare-commit-msg",
        "post-commit",
        "pre-merge-commit",
        "post-rewrite",
    ] {
        write(&hooks.join(h), "#!/bin/sh\nexit 1\n");
        common::executable(&hooks.join(h));
    }
    work.git(&cd, &["config", "core.hooksPath", hooks.to_str().unwrap()]);

    assert_ok(&work.stop_with(&[&cd], "{}", |c| {
        c.env("CLAUDECODE", "1");
    }));
    assert_eq!(
        work.git(
            &cd,
            &["show", &format!("{REF}:projects/-a/memory/MEMORY.md")]
        ),
        "index a"
    );
}

/// git の hook や dotfiles の別名から起動した Claude Code は、GIT_DIR などを継ぐ
#[test]
fn ignores_the_inherited_git_dir() {
    let work = Work::new();
    let cd = claude_dir(&work);
    let other = work.repo("other");

    assert_ok(&work.stop_with(&[&cd], "{}", |c| {
        c.env("GIT_DIR", other.join(".git"))
            .env("GIT_WORK_TREE", &other)
            .env("GIT_INDEX_FILE", other.join(".git/index"));
    }));
    assert_eq!(
        work.git(
            &cd,
            &["show", &format!("{REF}:projects/-a/memory/MEMORY.md")]
        ),
        "index a"
    );
    assert_eq!(work.backup(&other), "");
}

/// 同時に終わった応答の hook どうしで、片方の変更を失わない。各プロセスの前に別のファイルを足すので、最後にロックを
/// 取ったプロセスのツリーに全部が入る
#[test]
fn keeps_every_change_when_run_at_the_same_time() {
    let work = Work::new();
    let cd = claude_dir(&work);
    let head = work.git(&cd, &["rev-parse", "HEAD"]);
    let staged = work.git(&cd, &["diff", "--cached", "--name-status"]);

    let outs: Vec<Output> = std::thread::scope(|scope| {
        let handles: Vec<_> = (1..=6)
            .map(|i| {
                write(
                    &cd.join(format!("projects/-a/memory/p{i}.md")),
                    &format!("p{i}\n"),
                );
                let (work, cd) = (&work, &cd);
                scope.spawn(move || work.stop(&[cd], "{}"))
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for out in &outs {
        assert_ok(out);
    }

    let tree = work.git(&cd, &["ls-tree", "-r", "--name-only", REF]);
    for i in 1..=6 {
        assert!(
            tree.contains(&format!("projects/-a/memory/p{i}.md")),
            "{tree}"
        );
    }
    assert_eq!(
        work.git(&cd, &["rev-list", "--merges", "--count", REF]),
        "0"
    );
    let gitdir = PathBuf::from(work.git(&cd, &["rev-parse", "--absolute-git-dir"]));
    let mut left: Vec<String> = std::fs::read_dir(&gitdir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("memory-backup") && n != "memory-backup.lock")
        .collect();
    left.extend(
        std::fs::read_dir(work.path("tmp"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned()),
    );
    assert!(left.is_empty(), "一時の index が残った: {left:?}");
    assert_eq!(work.git(&cd, &["rev-parse", "HEAD"]), head);
    assert_eq!(
        work.git(&cd, &["diff", "--cached", "--name-status"]),
        staged
    );
}

/// git が起動して残る子（fsmonitor の daemon、detach する保守）がロックを持つと、退避の後もロックが解けず、次の応答の
/// 退避が待ちきれずに失敗する。fsmonitor の hook からバックグラウンドの sleep を残して確かめる
#[cfg(unix)]
#[test]
fn does_not_hand_the_lock_to_a_git_child_left_running() {
    let work = Work::new();
    let fsm = work.repo("fsm");
    let pids = work.path("fsmonitor.pids");
    let hook = work.path("fsmonitor.sh");
    write(
        &hook,
        &format!(
            "#!/bin/sh\nsleep 60 </dev/null >/dev/null 2>&1 &\necho $! >>'{}'\nexit 1\n",
            common::sh_path(&pids)
        ),
    );
    common::executable(&hook);
    work.git(&fsm, &["config", "core.fsmonitor", hook.to_str().unwrap()]);

    write(&fsm.join("projects/-a/memory/MEMORY.md"), "f1\n");
    let _ = work.stop(&[&fsm], "{}");
    write(&fsm.join("projects/-a/memory/MEMORY.md"), "f2\n");
    let start = std::time::Instant::now();
    let out = work.stop(&[&fsm], "{}");
    let waited = start.elapsed();
    if let Ok(text) = std::fs::read_to_string(&pids) {
        for pid in text.split_whitespace() {
            let _ = Command::new("kill").arg(pid).status();
        }
    }
    assert_ok(&out);
    assert!(waited.as_secs() < 10, "ロックを待った: {waited:?}");
    assert_eq!(
        work.git(
            &fsm,
            &["show", &format!("{REF}:projects/-a/memory/MEMORY.md")]
        ),
        "f2"
    );
}

/// ほかの退避がロックを持ったまま終わらなければ、30 秒待って、理由を出して失敗する
#[test]
fn fails_when_another_process_holds_the_lock() {
    let work = Work::new();
    let cd = claude_dir(&work);
    let gitdir = PathBuf::from(work.git(&cd, &["rev-parse", "--absolute-git-dir"]));
    let lock = std::fs::File::create(gitdir.join("memory-backup.lock")).unwrap();
    lock.lock().unwrap();

    let start = std::time::Instant::now();
    let out = work.stop(&[&cd], "{}");
    let waited = start.elapsed();
    drop(lock);

    let err = assert_failed(&out);
    assert!(err.contains("memory-backup.lock"), "{err}");
    assert!(waited.as_secs() >= 29, "待たずに失敗した: {waited:?}");
    assert_eq!(work.backup(&cd), "");
}

#[test]
fn succeeds_without_a_ref_when_there_is_no_memory() {
    let work = Work::new();
    let empty = work.repo("empty");
    assert_ok(&work.stop(&[&empty], "{}"));
    assert_eq!(work.backup(&empty), "");
}

#[test]
fn fails_outside_a_repository() {
    let work = Work::new();
    let norepo = work.path("norepo");
    write(&norepo.join("projects/-a/memory/MEMORY.md"), "x\n");
    let err = assert_failed(&work.stop(&[&norepo], "{}"));
    assert!(!err.contains("直下"), "外のリポジトリの下と読んだ: {err}");
}

/// 上の階層のリポジトリ（ホームを丸ごと追跡するものなど）に、知らずに ref を作らない
#[test]
fn fails_below_the_top_of_a_repository() {
    let work = Work::new();
    let top = work.repo("top");
    let cd = top.join("claude");
    write(&cd.join("projects/-a/memory/MEMORY.md"), "x\n");
    let err = assert_failed(&work.stop(&[&cd], "{}"));
    assert!(err.contains("直下"), "{err}");
    assert_eq!(work.backup(&top), "");
}

/// git add はリンク（mode 120000）だけを入れて中身を退避しないので、成功と見せない
#[cfg(unix)]
#[test]
fn fails_when_a_memory_directory_is_a_symlink() {
    let work = Work::new();
    let linked = work.repo("linked");
    write(&work.path("real-memory/MEMORY.md"), "real\n");
    std::fs::create_dir_all(linked.join("projects/-b")).unwrap();
    std::os::unix::fs::symlink(work.path("real-memory"), linked.join("projects/-b/memory"))
        .unwrap();

    let err = assert_failed(&work.stop(&[&linked], "{}"));
    assert!(err.contains("projects/-b/memory"), "{err}");
    assert_eq!(work.backup(&linked), "");
}

#[test]
fn uses_the_claude_dir_under_home_by_default() {
    let work = Work::new();
    let def = work.repo("home/.claude");
    write(&def.join("projects/-a/memory/MEMORY.md"), "def\n");
    assert_ok(&work.stop(&[], "{}"));
    assert_eq!(
        work.git(
            &def,
            &["show", &format!("{REF}:projects/-a/memory/MEMORY.md")]
        ),
        "def"
    );
}

/// 引数の誤りでも 2（応答を続けさせる）で終えない
#[test]
fn fails_with_1_on_extra_arguments() {
    let work = Work::new();
    let (a, b) = (work.path("a"), work.path("b"));
    let out = work.stop(&[&a, &b], "{}");
    let err = assert_failed(&out);
    assert!(err.contains("Usage: claude-hooks stop"), "{err}");
}

/// projects の下の UTF-8 でない名前（ほかのツールの残したファイルなど）で、ほかのプロジェクトの退避を止めない
#[cfg(unix)]
#[test]
fn backs_up_beside_a_name_that_is_not_utf8() {
    use std::os::unix::ffi::OsStrExt;
    let work = Work::new();
    let cd = claude_dir(&work);
    let odd = std::ffi::OsStr::from_bytes(b"-\xe9");
    write(&cd.join("projects").join(odd).join("memory/x.md"), "odd\n");

    assert_ok(&work.stop(&[&cd], "{}"));
    let out = work.git_output(&cd, &["ls-tree", "-r", "-z", "--name-only", REF]);
    let names: Vec<&[u8]> = out
        .stdout
        .split(|b| *b == 0)
        .filter(|n| !n.is_empty())
        .collect();
    assert!(
        names.contains(&&b"projects/-\xe9/memory/x.md"[..]),
        "{names:?}"
    );
    assert!(
        names.contains(&&b"projects/-a/memory/MEMORY.md"[..]),
        "{names:?}"
    );
}

// 最後の応答の言語の検査（ADR-0108）。英語で書いた応答だけを 2 で返し、日本語の応答は止めない

/// Stop の hook の入力（last_assistant_message と stop_hook_active）
fn stop_input(message: &str, active: bool) -> String {
    serde_json::json!({
        "session_id": "s1",
        "cwd": "/w",
        "hook_event_name": "Stop",
        "stop_hook_active": active,
        "last_assistant_message": message,
    })
    .to_string()
}

/// 英語の応答を返すときは、1 行目に理由を書いて 2 で終える（2 は応答を続けさせ、標準エラーが Claude に渡る）
fn assert_sent_back(out: &Output) -> String {
    assert_eq!(out.status.code(), Some(2), "{}", stderr(out));
    let err = stderr(out);
    assert!(
        err.lines()
            .next()
            .unwrap_or_default()
            .starts_with("claude-hooks stop: 最後の応答が日本語で書かれていません"),
        "1 行目が理由でない: {err}"
    );
    assert!(err.contains("日本語で書き直"), "修正方法が無い: {err}");
    err
}

const ENGLISH_REPORT: &str = "I've finished adding `pid` to the observation record. The fourth review round \
found no runtime bugs. It flagged one misplaced doc comment, which I fixed. The checks passed, and all 194 Rust \
tests pass.\n\nI'm waiting on two answers before I merge:\n\n1. **Merge as is?** Seven review findings are \
written up in issue 0025 instead of fixed here.";

#[test]
fn an_english_response_is_sent_back_and_the_memory_is_still_backed_up() {
    let work = Work::new();
    let cd = claude_dir(&work);
    assert_sent_back(&work.stop(&[&cd], &stop_input(ENGLISH_REPORT, false)));
    assert_eq!(
        work.git(&cd, &["ls-tree", "-r", "--name-only", REF]),
        "projects/-a/memory/MEMORY.md\nprojects/-b/memory/note.md"
    );
}

#[test]
fn an_english_response_quoting_a_short_japanese_phrase_is_sent_back() {
    let work = Work::new();
    let cd = claude_dir(&work);
    let message = "The user asked 「日本語で」 earlier, and I have now rewritten the summary of the \
analysis so that every section of the report follows the same structure as before.";
    assert_sent_back(&work.stop(&[&cd], &stop_input(message, false)));
}

#[test]
fn a_japanese_response_full_of_code_paths_and_identifiers_passes() {
    let work = Work::new();
    let cd = claude_dir(&work);
    let message = "`feat/stop-hook-japanese-response` を main にマージしました（PR #420、c0ffee12）。\n\n\
## 変えたもの\n\n\
- crates/claude-hooks/src/stop.rs: read_input が last_assistant_message と stop_hook_active を読む\n\
- C:\\Users\\u\\.claude\\bin\\claude-hooks.exe と ~/.claude/settings.json は変えていない\n\
- LockFileEx、File::try_lock、serde_json::Value、GIT_INDEX_FILE はそのまま\n\n\
```text\n\
running 42 tests\n\
test an_english_response_is_sent_back ... ok\n\
test result: ok. 42 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n\
```\n\n\
確認: https://github.com/example/repo/actions/runs/123 の Windows の CI が pass。`cargo test --test stop` も pass。";
    assert_ok(&work.stop(&[&cd], &stop_input(message, false)));
}

#[test]
fn a_japanese_response_quoting_a_long_english_passage_passes() {
    let work = Work::new();
    let cd = claude_dir(&work);
    let message = "hooks の文書の Stop の節には、次のように書かれています。\n\n\
> The `last_assistant_message` field contains the text content of Claude's final response, so hooks can \
access it without parsing the transcript file. For hooks that act on the just-completed turn, use this \
field rather than reading the transcript.\n\n\
なので、記録のファイルは読みません。";
    assert_ok(&work.stop(&[&cd], &stop_input(message, false)));
}

#[test]
fn a_short_english_response_passes() {
    let work = Work::new();
    let cd = claude_dir(&work);
    let message = "The checks passed. Waiting for the review.";
    assert_ok(&work.stop(&[&cd], &stop_input(message, false)));
}

#[test]
fn a_response_without_text_passes() {
    let work = Work::new();
    let cd = claude_dir(&work);
    assert_ok(&work.stop(&[&cd], &stop_input("", false)));
    assert_ok(&work.stop(&[&cd], r#"{"stop_hook_active":false}"#));
    assert_ok(&work.stop(
        &[&cd],
        r#"{"stop_hook_active":false,"last_assistant_message":null}"#,
    ));
}

/// 書き直させた後の Stop は、英語のままでも続けて止めない（直る見込みが小さく、手戻りだけが増える）
#[test]
fn a_response_after_a_stop_hook_continued_passes() {
    let work = Work::new();
    let cd = claude_dir(&work);
    assert_ok(&work.stop(&[&cd], &stop_input(ENGLISH_REPORT, true)));
}

/// 退避の失敗と英語の応答が重なったら、2 で返し、退避の失敗の行も Claude に渡す
#[test]
fn an_english_response_with_a_failed_backup_reports_both() {
    let work = Work::new();
    let norepo = work.path("norepo");
    write(&norepo.join("projects/-a/memory/MEMORY.md"), "x\n");
    let err = assert_sent_back(&work.stop(&[&norepo], &stop_input(ENGLISH_REPORT, false)));
    assert!(err.contains("コミットできませんでした"), "{err}");
}

/// ユーザーかスキルが英語の本文を求めた応答は、本文を訳させず、コードブロックか引用に入れて日本語の説明を添えさせる
/// （その形なら 0 で通る）
#[test]
#[ignore = "RED: 引用のブロックの書き方と文の空白は次のコミットで直す"]
fn the_send_back_tells_how_to_keep_a_requested_english_body() {
    let work = Work::new();
    let cd = claude_dir(&work);
    let err = assert_sent_back(&work.stop(&[&cd], &stop_input(ENGLISH_REPORT, false)));
    assert!(err.contains("英語の本文"), "{err}");
    assert!(!err.contains("  "), "文の途中に空白が続く: {err}");
    // 数えないのは `>` で始まる引用のブロックで、「」の引用は数える
    assert!(
        err.contains("コードブロックか `>` で始まる引用のブロック"),
        "{err}"
    );

    let kept = format!(
        "求められた英語の PR の本文です。英語のまま、コードブロックに入れて置きます。\n\n\
         ```markdown\n{ENGLISH_REPORT}\n```"
    );
    assert_ok(&work.stop(&[&cd], &stop_input(&kept, false)));
}
