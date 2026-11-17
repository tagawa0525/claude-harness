//! `claude-hooks script dictionary-preview --dictionary <辞書> [--base <辞書>]` が、セッションの記録のアシスタントの発言と
//! メモリに辞書の検査（ja-terms check）をかけ、置き換えを種類ごとに件数と例つきで出すことを確かめる
//! （`~/.claude/scripts/dictionary-preview.sh` が呼ぶ）。ja-terms は偽のコマンド（examples/fake_tool.rs）にする。
//! 本物の辞書での確認は `modules/home/parts/tests/ja-terms.sh` がする

mod common;

use common::{fake_tool, place_fake_tool};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Home {
    root: PathBuf,
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct Run {
    code: i32,
    out: String,
    err: String,
}

/// 記録の 1 行（アシスタントの発言。content は text の要素 1 つ）
fn assistant(text: &str) -> String {
    serde_json::json!({
        "type": "assistant",
        "message": {"content": [{"type": "text", "text": text}]}
    })
    .to_string()
}

impl Home {
    fn new() -> Home {
        assert!(
            fake_tool().is_file(),
            "{} が無い（cargo build --examples を先に流す）",
            fake_tool().display()
        );
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "dictionary-preview-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("projects/p1/memory")).unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        place_fake_tool(&root.join(format!("bin/ja-terms{}", std::env::consts::EXE_SUFFIX)));
        Home { root }
    }

    fn projects(&self) -> String {
        self.root.join("projects").display().to_string()
    }

    /// projects の下のファイルを書く（親のディレクトリも作る）
    fn write(&self, path: &str, text: &str) {
        let path = self.root.join("projects").join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.root.join("ja-terms.log")).unwrap_or_default()
    }

    /// 偽の ja-terms の n 回目の起動を、標準出力 outs[n-1].0 と終了コード outs[n-1].1 で終えるものにして実行する
    fn run(&self, args: &[&str], outs: &[(&str, i32)]) -> Run {
        let ja_terms = self
            .root
            .join(format!("bin/ja-terms{}", std::env::consts::EXE_SUFFIX));
        let mut command = Command::new(env!("CARGO_BIN_EXE_claude-hooks"));
        command
            .args(["script", "dictionary-preview"])
            .args(args)
            .env("CLAUDE_HOOKS_JA_TERMS", ja_terms)
            .env("FAKE_JA_TERMS_LOG", self.root.join("ja-terms.log"))
            .env("FAKE_JA_TERMS_CALLS", self.root.join("ja-terms.calls"));
        for (n, (out, exit)) in outs.iter().enumerate() {
            command
                .env(format!("FAKE_JA_TERMS_OUT_{}", n + 1), out)
                .env(format!("FAKE_JA_TERMS_EXIT_{}", n + 1), exit.to_string());
        }
        let output = command.output().unwrap();
        Run {
            code: output.status.code().unwrap(),
            out: String::from_utf8(output.stdout).unwrap(),
            err: String::from_utf8(output.stderr).unwrap(),
        }
    }

    /// --dictionary new.yml と --projects を付けて実行する
    fn preview(&self, more: &[&str], outs: &[(&str, i32)]) -> Run {
        let projects = self.projects();
        let mut args = vec!["--dictionary", "new.yml", "--projects", &projects];
        args.extend_from_slice(more);
        self.run(&args, outs)
    }
}

/// 発言が 3 行（1 行目と 3 行目に「写し」）の記録
fn three_utterances() -> Home {
    let h = Home::new();
    h.write(
        "p1/s1.jsonl",
        &format!(
            "{}\n{}\n",
            assistant("xlc の写しで試した結果です。\n設定を確認しました。"),
            assistant("入力の写しが 12 本残っています。")
        ),
    );
    h
}

#[test]
fn hits_in_utterances_are_grouped_with_counts_and_examples() {
    let h = three_utterances();
    // 発言は見出しの後に 1 行おきに置く（1 つ目は 3 行目、3 つ目は 7 行目）
    let r = h.preview(
        &[],
        &[(
            "utterances.md:3:6: 写し => コピー [Error/prh]\nutterances.md:7:4: 写し => コピー [Error/prh]\n",
            1,
        )],
    );
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(
        r.out,
        "SOURCES: transcripts=1 utterances=3 memory=0\n\
         HIT: 2 写し => コピー\n  \
         xlc の【写し】で試した結果です。\n  \
         入力の【写し】が 12 本残っています。\n"
    );
}

#[test]
fn ja_terms_gets_the_dictionary_the_utterances_and_the_memory_files() {
    let h = three_utterances();
    h.write("p1/memory/a.md", "本文\n");
    h.write("p1/memory/MEMORY.md", "- [a](a.md)\n");
    h.write("p2/memory/b.md", "本文\n");
    let r = h.preview(&[], &[("", 0)]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(r.out, "SOURCES: transcripts=1 utterances=3 memory=3\n");
    let p = h.projects();
    assert_eq!(
        h.log(),
        format!(
            "ja-terms check --dictionary new.yml utterances.md {p}/p1/memory/MEMORY.md {p}/p1/memory/a.md {p}/p2/memory/b.md\n"
        )
    );
}

#[test]
fn only_assistant_text_outside_fences_is_read_once() {
    let h = Home::new();
    let user = serde_json::json!({"type": "user", "message": {"content": "ユーザーの写し"}});
    let tool = serde_json::json!({
        "type": "assistant",
        "message": {"content": [{"type": "tool_use", "input": {"command": "道具の写し"}}]}
    });
    h.write(
        "p1/s1.jsonl",
        &format!(
            "{user}\n{tool}\n壊れた行\n{}\n",
            assistant("説明の写しです。\n\n```sh\nコードの写し\n```\n  説明の写しです。  \n~~~\nチルダの中\n~~~\n最後の行")
        ),
    );
    // サブエージェントの記録も読む。同じ行は 1 回だけ数える
    h.write(
        "p1/s1/subagents/agent-1.jsonl",
        &format!("{}\n", assistant("説明の写しです。\n担当の行")),
    );
    let r = h.preview(&[], &[("", 0)]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(r.out, "SOURCES: transcripts=2 utterances=3 memory=0\n");
}

#[test]
fn with_a_base_only_the_differences_are_shown() {
    let h = three_utterances();
    // 1 回目は新しい辞書、2 回目は比較元。3 行目の指摘は両方にあり、7 行目は新しい辞書だけ、5 行目は比較元だけ
    let r = h.preview(
        &["--base", "old.yml"],
        &[
            (
                "utterances.md:3:6: 写し => コピー [Error/prh]\nutterances.md:7:4: 写し => コピー [Error/prh]\n",
                1,
            ),
            (
                "utterances.md:3:6: 写し => コピー [Error/prh]\nutterances.md:5:4: 確認し => 確かめ [Error/prh]\n",
                1,
            ),
        ],
    );
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(
        r.out,
        "SOURCES: transcripts=1 utterances=3 memory=0\n\
         NEW: 1 写し => コピー\n  \
         入力の【写し】が 12 本残っています。\n\
         GONE: 1 確認し => 確かめ\n  \
         設定を【確認し】ました。\n"
    );
    let log = h.log();
    let calls: Vec<&str> = log.lines().collect();
    assert_eq!(calls.len(), 2, "{log}");
    assert!(calls[0].starts_with("ja-terms check --dictionary new.yml utterances.md"));
    assert!(calls[1].starts_with("ja-terms check --dictionary old.yml utterances.md"));
}

#[test]
fn kinds_are_ordered_by_count_and_examples_are_limited_and_distinct() {
    let h = Home::new();
    h.write(
        "p1/s1.jsonl",
        &format!(
            "{}\n",
            assistant(
                "写しを作る。\n道具を使う。\n写しを読む。\n写しを消す。\n写しを作る、写しを作る。"
            )
        ),
    );
    let r = h.preview(
        &["--examples", "2"],
        &[(
            "utterances.md:5:1: 道具 => ツール [Error/prh]\n\
             utterances.md:3:1: 写し => コピー [Error/prh]\n\
             utterances.md:7:1: 写し => コピー [Error/prh]\n\
             utterances.md:9:1: 写し => コピー [Error/prh]\n",
            1,
        )],
    );
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(
        r.out,
        "SOURCES: transcripts=1 utterances=5 memory=0\n\
         HIT: 3 写し => コピー\n  \
         【写し】を作る。\n  \
         【写し】を読む。\n\
         HIT: 1 道具 => ツール\n  \
         【道具】を使う。\n"
    );
}

#[test]
fn the_context_is_cut_to_twenty_characters_and_the_column_counts_utf16() {
    let h = Home::new();
    // 😀 は UTF-16 で 2 つ。「写し」は 31 文字目で、列は 32
    let line = format!("😀{}写し{}", "あ".repeat(29), "い".repeat(25));
    h.write("p1/s1.jsonl", &format!("{}\n", assistant(&line)));
    let r = h.preview(
        &[],
        &[("utterances.md:3:32: 写し => コピー [Error/prh]\n", 1)],
    );
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(
        r.out,
        format!(
            "SOURCES: transcripts=1 utterances=1 memory=0\nHIT: 1 写し => コピー\n  {}【写し】{}\n",
            "あ".repeat(20),
            "い".repeat(20)
        )
    );
}

#[test]
fn hits_in_memory_are_listed_with_their_places() {
    let h = three_utterances();
    h.write("p1/memory/a.md", "本文\n");
    h.write("p2/memory/b.md", "本文\n");
    let p = h.projects();
    let out = format!(
        "{p}/p1/memory/a.md:6:3: 道具 => ツール [Error/prh]\n{p}/p2/memory/b.md:2:1: 写し => コピー [Error/prh]\n"
    );
    // 比較元にもある指摘も、メモリは直す対象なので出す
    let r = h.preview(&["--base", "old.yml"], &[(&out, 1), (&out, 1)]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(
        r.out,
        "SOURCES: transcripts=1 utterances=3 memory=2\n\
         MEMORY: p1/memory/a.md:6 道具 => ツール\n\
         MEMORY: p2/memory/b.md:2 写し => コピー\n"
    );
}

#[test]
fn a_failing_check_stops_with_an_error() {
    let h = three_utterances();
    let r = h.preview(&[], &[("辞書を読めない", 2)]);
    assert_eq!(r.code, 1);
    assert!(r.err.contains("ERROR: 辞書の検査"), "{}", r.err);
    assert!(r.err.contains("終了コード 2"), "{}", r.err);
    // 崩れた語があると言いながら読める行が無ければ、出力の形が変わっている
    let h = three_utterances();
    let r = h.preview(&[], &[("読めない行\n", 1)]);
    assert_eq!(r.code, 1);
    assert!(r.err.contains("出力を読めません"), "{}", r.err);
}

#[test]
fn bad_arguments_show_the_usage() {
    let h = Home::new();
    for args in [
        vec![],
        vec!["--base", "old.yml"],
        vec!["--dictionary"],
        vec!["--dictionary", "new.yml", "--examples", "x"],
        vec!["--dictionary", "new.yml", "extra"],
    ] {
        let r = h.run(&args, &[]);
        assert_eq!(r.code, 1, "{args:?}");
        assert!(r.err.contains("Usage: dictionary-preview.sh"), "{}", r.err);
    }
    let r = h.run(
        &[
            "--dictionary",
            "new.yml",
            "--projects",
            "/nonexistent/projects",
        ],
        &[],
    );
    assert_eq!(r.code, 1);
    assert!(r.err.contains("ERROR: "), "{}", r.err);
    assert!(h.log().is_empty());
}
