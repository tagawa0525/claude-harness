//! `claude-hooks script session-events [--since <日時>] [--until <日時>] [TRANSCRIPT]` が、会話の記録から振り返りの
//! 手がかり（拒否、失敗、ユーザーの発言）を集めることを確かめる（ADR-0089。`~/.claude/skills/retro/scripts/session-events.sh`
//! が呼ぶ）。sh から呼ぶ配備の確認は `.claude/tests/scripts/retro.sh`

mod common;

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const SESSION: &str = "11111111-2222-3333-4444-555555555555";

struct Home {
    root: PathBuf,
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn user_text(text: &str) -> Value {
    json!({"type": "user", "message": {"role": "user", "content": text}})
}

/// 失敗したツールの結果の行
fn tool_error(content: &str) -> Value {
    json!({"type": "user", "message": {"role": "user", "content": [
        {"type": "tool_result", "tool_use_id": "t", "is_error": true, "content": content}]}})
}

fn tool_ok(content: &str) -> Value {
    json!({"type": "user", "message": {"role": "user", "content": [
        {"type": "tool_result", "tool_use_id": "t", "content": content}]}})
}

fn tool_use() -> Value {
    json!({"type": "assistant", "message": {"role": "assistant", "content": [
        {"type": "tool_use", "id": "t", "name": "Bash", "input": {}}]}})
}

fn queued(prompt: &str, origin: Option<&str>) -> Value {
    let mut a = json!({"type": "queued_command", "prompt": prompt, "commandMode": "prompt"});
    if let Some(kind) = origin {
        a["origin"] = json!({"kind": kind});
    }
    json!({"type": "attachment", "attachment": a})
}

fn at(mut line: Value, ts: &str) -> Value {
    line["timestamp"] = json!(ts);
    line
}

const GIT_ADD: &str =
    "PreToolUse:Bash hook error: git add -A は禁止です\ngit add <file> でステージしてください";

impl Home {
    fn new() -> Home {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "session-events-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Home { root }
    }

    fn project(&self) -> PathBuf {
        self.root
            .join(".claude")
            .join("projects")
            .join("-home-u-repo")
    }

    fn write_lines(&self, path: &Path, lines: &[Value]) -> String {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
        std::fs::write(path, text).unwrap();
        path.display().to_string()
    }

    /// sh のテストと同じ記録（本体とサブエージェント 1 本）を置き、本体のパスを返す
    fn first_session(&self) -> String {
        let main = self.project().join(format!("{SESSION}.jsonl"));
        let sub = self.project().join(SESSION).join("subagents");
        self.write_lines(
            &sub.join("agent-a1.jsonl"),
            &[
                user_text("親からの依頼文"),
                tool_error(GIT_ADD),
                json!({"type": "user", "message": {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "t", "is_error": true, "content": [
                        {"type": "text", "text": "The user doesn't want to proceed with this tool use. The tool use was rejected."}]}]}}),
            ],
        );
        self.write_lines(
            &main,
            &[
                json!({"type": "system", "subtype": "turn_duration"}),
                user_text("PR を作ってください"),
                tool_error(GIT_ADD),
                tool_error(GIT_ADD),
                tool_error("Permission for this action was denied by the Claude Code auto mode classifier. Reason: [Git Destructive]."),
                tool_error("Exit code 1\nerror: pathspec 'x' did not match"),
                tool_error("<tool_use_error>File has not been read yet.</tool_use_error>"),
                tool_error("PreToolUse:Bash hook error: 最後の push (e7c9939) は自動レビューを受けていません（3 件）"),
                tool_error("PreToolUse:Bash hook error: 最後の push (71cfb82) は自動レビューを受けていません（12 件）"),
                tool_error("The server-side auto mode classifier gave no verdict for Bash: rm -rf x"),
                tool_error("PreToolUse:Bash hook error: ADR-0015 により止めます。コミットe7c9939dは約10分前"),
                tool_error("Exit code 1\n✗ fixture: missing 'auto mode classifier'"),
                tool_error("Exit code 2\nSKILL.md\ngrep: x: No such file or directory\nSKILL.md"),
                tool_error("Exit code 127\npassed: 237, failed: 0\nbash: command not found: shellcheck"),
                tool_ok("成功の印"),
                tool_error("Error: something happened in auto mode classifier test"),
                tool_error("Permission to use Bash with command rm -rf x has been denied."),
                tool_error("Permission for this command was denied by a built-in safety check."),
                queued("別のエージェントの言葉", Some("agent")),
                tool_error("The user doesn't want to proceed with this tool use. The tool use was rejected. To tell you how to proceed, the user said:\nsquash しないで\nno-ff にして"),
                json!({"type": "user", "message": {"role": "user", "content": [
                    {"type": "text", "text": "<ide_selection>選んだ行</ide_selection>"},
                    {"type": "text", "text": "IDE から送った訂正"}]}}),
                queued("途中で送った訂正", Some("human")),
                queued("<task-notification>x</task-notification>", None),
                json!({"type": "user", "isCompactSummary": true, "isVisibleInTranscriptOnly": true,
                    "message": {"role": "user", "content": "This session is being continued"}}),
                user_text("<command-message>gh-pr-merge</command-message>\n<command-name>/gh-pr-merge</command-name>\n<command-args>371</command-args>"),
                user_text("<task-notification>\n<task-id>b1</task-id>"),
                json!({"type": "user", "isMeta": true, "message": {"role": "user", "content": "スキルの本文"}}),
                user_text("[Request interrupted by user]"),
                user_text("squash しないで\nno-ff でマージしてください"),
                json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "text", "text": "はい"}]}}),
            ],
        )
    }

    /// 区間のテストの記録（本体、区間に行のあるサブエージェント、区間の前だけのサブエージェント）
    fn range_session(&self) -> String {
        let main = self.root.join("range").join("s.jsonl");
        let sub = self.root.join("range").join("s").join("subagents");
        self.write_lines(
            &sub.join("agent-in.jsonl"),
            &[at(
                tool_error("PreToolUse:Bash hook error: 区間の拒否"),
                "2026-11-05T16:00:00.000Z",
            )],
        );
        self.write_lines(
            &sub.join("agent-out.jsonl"),
            &[at(
                tool_error("PreToolUse:Bash hook error: サブエージェントの区間の前の拒否"),
                "2026-11-05T14:00:00.000Z",
            )],
        );
        self.write_lines(
            &main,
            &[
                at(user_text("区間の前の発言"), "2026-11-05T14:59:59.999Z"),
                at(
                    tool_error("PreToolUse:Bash hook error: 区間の前の拒否"),
                    "2026-11-05T14:59:59.999Z",
                ),
                at(user_text("区間の始めの発言"), "2026-11-05T15:00:00.000Z"),
                at(
                    tool_error("PreToolUse:Bash hook error: 区間の拒否"),
                    "2026-11-05T15:00:00.000Z",
                ),
                at(
                    tool_error("PreToolUse:Bash hook error: 区間の拒否"),
                    "2026-11-06T02:04:59.999Z",
                ),
                user_text("時刻の無い発言"),
                at(user_text("区間の終わりの発言"), "2026-11-06T02:05:00.000Z"),
            ],
        )
    }

    fn run_in(&self, args: &[&str], session: Option<&str>, dir: Option<&Path>) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_claude-hooks"));
        cmd.args(["script", "session-events"])
            .args(args)
            .env("HOME", &self.root)
            .env_remove("CLAUDE_CODE_SESSION_ID");
        if let Some(session) = session {
            cmd.env("CLAUDE_CODE_SESSION_ID", session);
        }
        if let Some(dir) = dir {
            cmd.current_dir(dir);
        }
        cmd.output().unwrap()
    }

    fn run(&self, args: &[&str]) -> Output {
        self.run_in(args, None, None)
    }
}

fn stdout(out: &Output) -> String {
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).unwrap()
}

/// 1 で終え、標準出力に何も出さず、標準エラーが `prefix` で始まる行を含む
fn assert_fails(out: &Output, prefix: &str) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.lines().any(|l| l.starts_with(prefix)),
        "{prefix}: {stderr}"
    );
    assert!(
        out.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

fn lines_with<'a>(out: &'a str, prefix: &str) -> Vec<&'a str> {
    out.lines().filter(|l| l.starts_with(prefix)).collect()
}

#[test]
fn failures_are_counted_by_kind_and_summary_and_user_messages_follow_in_order() {
    let home = Home::new();
    let main = home.first_session();
    let out = stdout(&home.run(&[&main]));
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        [
            format!("TRANSCRIPT: {main}").as_str(),
            // 本体とサブエージェント
            "TRANSCRIPTS: 2",
            // hook の拒否は `<event>:<tool> hook error: ` を除いた 1 行目を、サブエージェントの分も合わせて数える
            "HOOK: 3 git add -A は禁止です",
            // コミットの番号と数を置き換えて、同じ拒否をまとめる
            "HOOK: 2 最後の push (<hash>) は自動レビューを受けていません（N 件）",
            // `-` に続く数は残し、日本語に接したコミットの番号も置き換える
            "HOOK: 1 ADR-0015 により止めます。コミット<hash>は約N分前",
            // 同じ回数の中は本文のバイトの順
            "PERMISSION: 1 Permission for this action was denied by the Claude Code auto mode classifier. Reason: [Git Destructive].",
            "PERMISSION: 1 Permission for this command was denied by a built-in safety check.",
            "PERMISSION: 1 Permission to use Bash with command rm -rf x has been denied.",
            "PERMISSION: 1 The server-side auto mode classifier gave no verdict for Bash: rm -rf x",
            "REJECTED: 1 The user doesn't want to proceed with this tool use.",
            // ユーザーが止めたときに書いた言葉は、改行を空白にして残す
            "REJECTED: 1 the user said: squash しないで no-ff にして",
            // 1 行目の途中に分類器の語があるだけの失敗は権限の拒否に数えない
            "TOOL: 1 Error: something happened in auto mode classifier test",
            // 終了コードのある失敗は、エラーらしい行の最後（`failed: 0` は数えない）、無ければ最後の行とつなぐ
            "TOOL: 1 Exit code 127: bash: command not found: shellcheck",
            "TOOL: 1 Exit code 1: error: pathspec 'x' did not match",
            "TOOL: 1 Exit code 1: ✗ fixture: missing 'auto mode classifier'",
            "TOOL: 1 Exit code 2: grep: x: No such file or directory",
            // `<tool_use_error>` のタグは除く
            "TOOL: 1 File has not been read yet.",
            // 本体の発言だけを順に。タグだけのブロック、人以外から届いた queued_command、通知、要約、isMeta は除き、
            // スラッシュコマンドは `/<名前> <引数>` にする
            "USER: PR を作ってください",
            "USER: IDE から送った訂正",
            "USER: 途中で送った訂正",
            "USER: /gh-pr-merge 371",
            "USER: [Request interrupted by user]",
            "USER: squash しないで no-ff でマージしてください",
        ]
    );
}

#[test]
fn failures_are_ordered_by_kind_then_by_count() {
    let home = Home::new();
    let main = home.write_lines(
        &home.root.join("order.jsonl"),
        &[
            tool_error("b"),
            tool_error("a"),
            tool_error("b"),
            tool_error("The user doesn't want to proceed with this tool use. x"),
            tool_error("PreToolUse:Bash hook error: h"),
        ],
    );
    let out = stdout(&home.run(&[&main]));
    assert_eq!(
        lines_with(&out, "")[2..],
        [
            "HOOK: 1 h",
            "REJECTED: 1 The user doesn't want to proceed with this tool use.",
            "TOOL: 2 b",
            "TOOL: 1 a",
        ]
    );
}

#[test]
fn without_a_transcript_the_session_of_claude_code_session_id_is_read() {
    let home = Home::new();
    let main = home.first_session();
    let out = stdout(&home.run_in(&[], Some(SESSION), None));
    assert_eq!(
        lines_with(&out, "TRANSCRIPT: "),
        [format!("TRANSCRIPT: {main}")]
    );
}

#[test]
fn without_a_transcript_or_a_session_it_fails() {
    let home = Home::new();
    home.first_session();
    assert_fails(&home.run(&[]), "ERROR: 記録のパスを渡すか");
    assert_fails(
        &home.run_in(&[], Some(""), None),
        "ERROR: 記録のパスを渡すか",
    );
}

#[test]
fn a_session_without_a_transcript_fails() {
    let home = Home::new();
    home.first_session();
    assert_fails(
        &home.run_in(&[], Some("no-such-session"), None),
        "ERROR: セッション no-such-session の記録が 0 件見つかりました（1 件のはず）",
    );
}

#[test]
fn an_unreadable_transcript_fails() {
    let home = Home::new();
    let missing = home.root.join("missing.jsonl").display().to_string();
    assert_fails(
        &home.run(&[&missing]),
        &format!("ERROR: 記録を読めません: {missing}"),
    );
}

#[test]
fn a_transcript_without_typed_lines_fails_instead_of_reporting_nothing() {
    let home = Home::new();
    let path = home.write_lines(
        &home.root.join("unknown.jsonl"),
        &[json!({"kind": "other"})],
    );
    assert_fails(&home.run(&[&path]), "ERROR: type を持つ行がありません");
}

#[test]
fn tool_uses_without_any_tool_result_fail_as_a_changed_format() {
    let home = Home::new();
    let path = home.write_lines(
        &home.root.join("renamed.jsonl"),
        &[
            tool_use(),
            json!({"type": "user", "message": {"role": "user", "content": [
                {"type": "result", "tool_use_id": "t", "content": "x"}]}}),
        ],
    );
    assert_fails(
        &home.run(&[&path]),
        "ERROR: ツールの呼び出しが 1 件あるのに tool_result がありません",
    );
}

#[test]
fn a_tool_result_outside_user_lines_does_not_pass_the_format_check() {
    let home = Home::new();
    let path = home.write_lines(
        &home.root.join("blob.jsonl"),
        &[
            tool_use(),
            json!({"type": "api-request-blob", "message": {"content": [
                {"type": "tool_result", "tool_use_id": "t", "content": "x"}]}}),
        ],
    );
    assert_fails(&home.run(&[&path]), "ERROR: ツールの呼び出しが 1 件");
}

#[test]
fn tool_results_of_subagents_pass_the_format_check() {
    let home = Home::new();
    let main = home.write_lines(&home.root.join("s.jsonl"), &[tool_use()]);
    home.write_lines(
        &home.root.join("s").join("subagents").join("agent-a.jsonl"),
        &[tool_ok("x")],
    );
    let out = stdout(&home.run(&[&main]));
    assert_eq!(lines_with(&out, "TRANSCRIPTS: "), ["TRANSCRIPTS: 2"]);
}

#[test]
fn a_cut_last_line_and_lines_that_are_not_json_objects_are_skipped() {
    let home = Home::new();
    let path = home.root.join("partial.jsonl");
    let mut text = String::from("not json\n[1]\n\n");
    text += &format!("{}\n", tool_error(GIT_ADD));
    // 書きかけの最後の行は文字の途中で切れることもある
    let mut bytes = text.into_bytes();
    bytes.extend_from_slice(
        br#"{"type":"assistant","message":{"content":[{"type":"tool_use","text":""#,
    );
    bytes.extend_from_slice(&"あ".as_bytes()[..2]);
    std::fs::write(&path, bytes).unwrap();
    let out = stdout(&home.run(&[path.to_str().unwrap()]));
    assert_eq!(
        lines_with(&out, "HOOK: "),
        ["HOOK: 1 git add -A は禁止です"]
    );
}

#[test]
fn a_relative_transcript_starting_with_a_dash_is_read() {
    let home = Home::new();
    let main = home.first_session();
    let rel = home.root.join("rel");
    std::fs::create_dir_all(&rel).unwrap();
    std::fs::copy(&main, rel.join("-home-u.jsonl")).unwrap();
    let out = stdout(&home.run_in(&["-home-u.jsonl"], None, Some(&rel)));
    assert_eq!(
        lines_with(&out, "TRANSCRIPT: "),
        ["TRANSCRIPT: ./-home-u.jsonl"]
    );
    assert_eq!(
        lines_with(&out, "HOOK: ")[0],
        "HOOK: 2 git add -A は禁止です"
    );
}

#[test]
fn numbers_without_letters_are_numbers_and_long_hex_runs_are_not_hashes() {
    let home = Home::new();
    let hex41 = "a".repeat(40) + "1";
    let path = home.write_lines(
        &home.root.join("big.jsonl"),
        &[
            tool_error("PreToolUse:Bash hook error: 大きさ 10485760 を超えています"),
            tool_error(&format!(
                "PreToolUse:Bash hook error: {hex41} abc1234g #12 x12 deadbee1 1a"
            )),
        ],
    );
    let out = stdout(&home.run(&[&path]));
    assert_eq!(
        lines_with(&out, "HOOK: "),
        [
            format!("HOOK: 1 {hex41} abc1234g #12 x12 <hash> Na").as_str(),
            "HOOK: 1 大きさ N を超えています",
        ]
    );
}

#[test]
fn a_hook_error_is_found_only_at_the_start_of_the_body() {
    let home = Home::new();
    let path = home.write_lines(
        &home.root.join("hook.jsonl"),
        &[tool_error("前の行\nPreToolUse:Bash hook error: x")],
    );
    let out = stdout(&home.run(&[&path]));
    assert_eq!(lines_with(&out, "TOOL: "), ["TOOL: 1 前の行"]);
}

#[test]
fn long_summaries_are_cut_at_160_characters_and_messages_at_300() {
    let home = Home::new();
    let path = home.write_lines(
        &home.root.join("long.jsonl"),
        &[tool_error(&"あ".repeat(300)), user_text(&"い".repeat(400))],
    );
    let out = stdout(&home.run(&[&path]));
    assert_eq!(
        lines_with(&out, "TOOL: "),
        [format!("TOOL: 1 {}", "あ".repeat(160))]
    );
    assert_eq!(
        lines_with(&out, "USER: "),
        [format!("USER: {}", "い".repeat(300))]
    );
}

#[test]
fn bodies_without_text_and_block_lists_are_summarized() {
    let home = Home::new();
    let path = home.write_lines(
        &home.root.join("body.jsonl"),
        &[
            tool_error(" \n "),
            tool_error("Exit code 3\n"),
            json!({"type": "user", "message": {"content": [{"type": "tool_result", "is_error": true,
                "content": [{"type": "image"}, {"type": "text", "text": "\nblock text"}]}]}}),
            json!({"type": "user", "message": {"content": [{"type": "tool_result", "is_error": true,
                "content": {"b": 1, "a": "\u{7f}"}}]}}),
        ],
    );
    let out = stdout(&home.run(&[&path]));
    assert_eq!(
        lines_with(&out, "TOOL: "),
        [
            "TOOL: 1 (本文なし)",
            "TOOL: 1 Exit code 3: (本文なし)",
            "TOOL: 1 block text",
            // 文字列でない本文は jq の tojson と同じ形（作った順のキー、DEL の escape）
            r#"TOOL: 1 {"b":1,"a":"\u007f"}"#,
        ]
    );
}

#[test]
fn a_rejection_without_words_keeps_the_sentence_up_to_the_tool_use() {
    let home = Home::new();
    let path = home.write_lines(
        &home.root.join("rejected.jsonl"),
        &[
            tool_error(
                "The user doesn't want to proceed with this tool use. The tool use was rejected.",
            ),
            tool_error("The user doesn't want to proceed now"),
        ],
    );
    let out = stdout(&home.run(&[&path]));
    assert_eq!(
        lines_with(&out, "REJECTED: "),
        [
            "REJECTED: 1 The user doesn't want to proceed now",
            "REJECTED: 1 The user doesn't want to proceed with this tool use.",
        ]
    );
}

#[test]
fn a_command_without_arguments_or_with_arguments_over_lines_shows_only_its_name() {
    let home = Home::new();
    let path = home.write_lines(
        &home.root.join("command.jsonl"),
        &[
            user_text("<command-name>/retro</command-name>"),
            user_text("<command-name>/x</command-name>\n<command-args>a\nb</command-args>"),
            user_text("<command-name>/broken"),
            queued("<command-name>/q</command-name> そのまま", Some("human")),
        ],
    );
    let out = stdout(&home.run(&[&path]));
    // 閉じていない command-name は発言にしない。queued_command は名前を取り出さない（タグで始まるので除く）
    assert_eq!(lines_with(&out, "USER: "), ["USER: /retro", "USER: /x"]);
}

#[test]
fn user_flags_other_than_false_exclude_the_message() {
    let home = Home::new();
    let mut flagged = user_text("印のある発言");
    flagged["isMeta"] = json!(0);
    let mut unflagged = user_text("印の無い発言");
    unflagged["isMeta"] = Value::Null;
    unflagged["isSidechain"] = json!(false);
    let path = home.write_lines(&home.root.join("flags.jsonl"), &[flagged, unflagged]);
    let out = stdout(&home.run(&[&path]));
    assert_eq!(lines_with(&out, "USER: "), ["USER: 印の無い発言"]);
}

#[test]
fn a_user_line_of_an_unknown_shape_fails_instead_of_being_skipped() {
    let home = Home::new();
    for (name, line) in [
        ("message", json!({"type": "user", "message": "x"})),
        (
            "content",
            json!({"type": "user", "message": {"role": "user"}}),
        ),
        (
            "block",
            json!({"type": "user", "message": {"content": ["x"]}}),
        ),
        (
            "text",
            json!({"type": "user", "message": {"content": [{"type": "text", "text": 1}]}}),
        ),
    ] {
        let path = home.write_lines(&home.root.join(format!("{name}.jsonl")), &[line]);
        assert_fails(&home.run(&[&path]), "ERROR: ");
    }
}

#[test]
fn the_range_counts_lines_of_the_transcript_and_subagents_in_since_until() {
    let home = Home::new();
    let main = home.range_session();
    let out = stdout(&home.run(&[
        "--since",
        "2026-11-06T00:00+09:00",
        "--until",
        "2026-11-06T11:05+09:00",
        &main,
    ]));
    assert_eq!(
        out.lines().collect::<Vec<_>>(),
        [
            format!("TRANSCRIPT: {main}").as_str(),
            // 区間の端を UTC に直す
            "SINCE: 2026-11-05T15:00:00.000Z",
            "UNTIL: 2026-11-06T02:05:00.000Z",
            // 区間に行の無いサブエージェントの記録は数えない
            "TRANSCRIPTS: 2",
            "HOOK: 3 区間の拒否",
            "USER: 区間の始めの発言",
        ]
    );
    // Z で書いた区間も同じ行を数える
    let z = stdout(&home.run(&[
        "--since",
        "2026-11-05T15:00Z",
        "--until",
        "2026-11-06T02:05:00.000Z",
        &main,
    ]));
    assert_eq!(out, z);
}

#[test]
fn one_bound_leaves_the_other_end_open() {
    let home = Home::new();
    let main = home.range_session();
    let out = stdout(&home.run(&["--since", "2026-11-05T15:00Z", &main]));
    assert!(lines_with(&out, "UNTIL: ").is_empty(), "{out}");
    assert_eq!(
        lines_with(&out, "USER: "),
        ["USER: 区間の始めの発言", "USER: 区間の終わりの発言"]
    );
    let out = stdout(&home.run(&["--until", "2026-11-06T02:05Z", &main]));
    assert_eq!(lines_with(&out, "TRANSCRIPTS: "), ["TRANSCRIPTS: 3"]);
    assert_eq!(
        lines_with(&out, "HOOK: "),
        [
            "HOOK: 3 区間の拒否",
            "HOOK: 1 サブエージェントの区間の前の拒否",
            "HOOK: 1 区間の前の拒否",
        ]
    );
    // timestamp の無い行は、区間を指定すると数えない
    assert_eq!(
        lines_with(&out, "USER: "),
        ["USER: 区間の前の発言", "USER: 区間の始めの発言"]
    );
}

#[test]
fn without_a_range_lines_without_a_timestamp_are_counted() {
    let home = Home::new();
    let main = home.range_session();
    let out = stdout(&home.run(&[&main]));
    assert!(lines_with(&out, "SINCE: ").is_empty(), "{out}");
    assert_eq!(lines_with(&out, "TRANSCRIPTS: "), ["TRANSCRIPTS: 3"]);
    assert!(out.contains("USER: 時刻の無い発言\n"), "{out}");
}

#[test]
fn the_fraction_of_a_bound_is_cut_to_milliseconds() {
    let home = Home::new();
    let main = home.range_session();
    let out = stdout(&home.run(&["--since", "2026-11-05T23:59:59.99999-01:00", &main]));
    assert_eq!(
        lines_with(&out, "SINCE: "),
        ["SINCE: 2026-11-06T00:59:59.999Z"]
    );
}

#[test]
fn a_bound_without_an_offset_or_unreadable_fails() {
    let home = Home::new();
    let main = home.range_session();
    for v in [
        "2026-11-06T00:00",
        "2026-11-06",
        "2026-11-06 00:00+09:00",
        "yesterday",
    ] {
        assert_fails(
            &home.run(&["--since", v, &main]),
            &format!(
                "ERROR: --since は時差つきの ISO 8601 で書いてください（例 2026-11-06T00:00+09:00、2026-11-05T15:00Z）: {v}"
            ),
        );
    }
    assert_fails(
        &home.run(&["--until", "2026-13-40T00:00Z", &main]),
        "ERROR: --until の日時を読めません: 2026-13-40T00:00Z",
    );
}

#[test]
fn since_not_before_until_fails() {
    let home = Home::new();
    let main = home.range_session();
    assert_fails(
        &home.run(&[
            "--since",
            "2026-11-06T02:05Z",
            "--until",
            "2026-11-06T11:05+09:00",
            &main,
        ]),
        "ERROR: --since（2026-11-06T02:05:00.000Z）は --until（2026-11-06T02:05:00.000Z）より前にしてください",
    );
}

#[test]
fn a_range_without_lines_fails_as_a_mistaken_range() {
    let home = Home::new();
    let main = home.range_session();
    assert_fails(
        &home.run(&[
            "--since",
            "2025-11-06T00:00+09:00",
            "--until",
            "2025-11-06T11:05+09:00",
            &main,
        ]),
        "ERROR: 区間 [2025-11-05T15:00:00.000Z, 2025-11-06T02:05:00.000Z) に timestamp の入る行が、本体にもサブエージェントにもありません",
    );
    assert_fails(
        &home.run(&["--since", "2027-01-01T00:00Z", &main]),
        "ERROR: 区間 [2027-01-01T00:00:00.000Z, -) に",
    );
}

#[test]
fn lines_of_a_subagent_count_when_the_transcript_has_none_in_the_range() {
    let home = Home::new();
    let main = home.range_session();
    let out = stdout(&home.run(&[
        "--since",
        "2026-11-05T15:30Z",
        "--until",
        "2026-11-05T16:30Z",
        &main,
    ]));
    assert_eq!(lines_with(&out, "HOOK: "), ["HOOK: 1 区間の拒否"]);
    assert_eq!(lines_with(&out, "TRANSCRIPTS: "), ["TRANSCRIPTS: 2"]);
}

#[test]
fn a_timestamp_of_another_format_fails_when_a_range_is_given() {
    let home = Home::new();
    let path = home.write_lines(
        &home.root.join("range-format.jsonl"),
        &[
            at(user_text("発言"), "2026-11-05T15:00:00.000Z"),
            at(user_text("秒の無い発言"), "2026-11-05T15:00Z"),
        ],
    );
    let out = home.run(&["--since", "2026-11-05T14:00Z", &path]);
    assert_fails(
        &out,
        &format!(
            "ERROR: timestamp の形が違います（記録の形が変わったかもしれません）: {path}: \"2026-11-05T15:00Z\""
        ),
    );
    assert_fails(&out, "ERROR: 区間で絞れませんでした");
    // 区間を指定しなければ形を見ない
    stdout(&home.run(&[&path]));
}

#[test]
fn a_missing_option_value_an_unknown_option_or_two_transcripts_show_the_usage() {
    let home = Home::new();
    let main = home.range_session();
    for args in [
        vec![main.as_str(), "--since"],
        vec!["--from", "2026-11-06T00:00Z", &main],
        vec!["--since=2026-11-06T00:00Z", &main],
        vec![&main, &main],
    ] {
        assert_fails(
            &home.run(&args),
            "Usage: session-events.sh [--since <日時>] [--until <日時>] [TRANSCRIPT]",
        );
    }
}
