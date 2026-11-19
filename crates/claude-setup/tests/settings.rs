//! `claude-setup settings` が、~/.claude/settings.json に静的な設定を重ね、ハーネスの hook の登録を 1 件ずつに揃える
//! こと（ADR-0084。前の claude-code.nix の activation の jq の filter と同じ規則で、登録は exec form）。

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

struct Home(PathBuf);

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Home {
    fn new() -> Home {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
            "settings-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        Home(dir)
    }

    fn settings(&self) -> PathBuf {
        self.0.join(".claude/settings.json")
    }

    fn write_settings(&self, value: &Value) {
        std::fs::write(
            self.settings(),
            serde_json::to_string_pretty(value).unwrap(),
        )
        .unwrap();
    }

    fn read_settings(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.settings()).unwrap()).unwrap()
    }

    /// 静的な設定のファイルを書いて、settings を実行する
    fn run(&self, statics: &Value, args: &[&str]) -> Output {
        let path = self.0.join("static.json");
        std::fs::write(&path, statics.to_string()).unwrap();
        Command::new(env!("CARGO_BIN_EXE_claude-setup"))
            .env("HOME", &self.0)
            .env("USERPROFILE", &self.0)
            .arg("settings")
            .arg("--static")
            .arg(&path)
            .args(args)
            .output()
            .unwrap()
    }

    fn ok(&self, statics: &Value, args: &[&str]) -> Value {
        let out = self.run(statics, args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        self.read_settings()
    }
}

const HOOKS: &str = "/home/u/.claude/bin/claude-hooks";
const DATE: &str = "date '+現在時刻: %F %H:%M %Z'";

fn pre_tool_use() -> Value {
    json!({"matcher": "Bash|Agent|Skill|Edit|Write|SendMessage|SubagentHandback", "hooks": [
        {"type": "command", "command": HOOKS, "args": ["pre-tool-use"], "timeout": 60}
    ]})
}

fn stop() -> Value {
    json!({"hooks": [{"type": "command", "command": HOOKS, "args": ["stop"], "timeout": 60}]})
}

fn date() -> Value {
    json!({"hooks": [{"type": "command", "command": DATE}]})
}

#[test]
fn a_missing_settings_file_gets_the_static_settings_and_the_hooks() {
    let home = Home::new();
    let got = home.ok(
        &json!({"language": "Japanese"}),
        &["--pre-tool-use", HOOKS, "--stop", HOOKS],
    );
    assert_eq!(
        got,
        json!({
            "language": "Japanese",
            "hooks": {
                "PreToolUse": [pre_tool_use()],
                "UserPromptSubmit": [date()],
                "Stop": [stop()],
            }
        })
    );
}

#[test]
fn static_settings_replace_top_level_keys_and_keep_the_others() {
    let home = Home::new();
    home.write_settings(&json!({
        "model": "opus",
        "permissions": {"allow": ["Bash(ls:*)"], "deny": ["x"]},
        "theme": "dark",
    }));
    let got = home.ok(&json!({"permissions": {"allow": ["Bash(git:*)"]}}), &[]);
    // jq の `. + $static` と同じく、いちばん上のキーごとに置き換える
    assert_eq!(got["permissions"], json!({"allow": ["Bash(git:*)"]}));
    assert_eq!(got["model"], json!("opus"));
    assert_eq!(got["theme"], json!("dark"));
}

#[test]
fn the_old_registrations_are_replaced_and_other_hooks_are_kept() {
    let home = Home::new();
    let other = json!({"type": "command", "command": "/opt/other-hook"});
    home.write_settings(&json!({"hooks": {
        "PreToolUse": [
            {"matcher": "Bash", "hooks": [
                {"type": "command", "command": "/home/u/.claude/hooks/guard-git-add.sh"},
                other,
            ]},
            {"matcher": "Bash|Agent|Skill|Edit|Write", "hooks": [
                {"type": "command", "command": "/home/u/.claude/bin/claude-hooks pre-tool-use", "timeout": 60}
            ]},
        ],
        "UserPromptSubmit": [
            {"hooks": [{"type": "command", "command": "date '+現在時刻: %F'"}]},
            {"hooks": [{"type": "command", "command": "/opt/prompt-hook"}]},
        ],
        "Stop": [{"hooks": [
            {"type": "command", "command": "/home/u/.claude/bin/claude-memory-backup", "timeout": 30}
        ]}],
        "SessionEnd": [{"hooks": [{"type": "command", "command": "/opt/end"}]}],
    }}));
    let got = home.ok(
        &json!({}),
        &[
            "--pre-tool-use",
            HOOKS,
            "--stop",
            HOOKS,
            "--legacy",
            "guard-git-add.sh",
        ],
    );
    assert_eq!(
        got["hooks"]["PreToolUse"],
        json!([{"matcher": "Bash", "hooks": [other]}, pre_tool_use()])
    );
    assert_eq!(
        got["hooks"]["UserPromptSubmit"],
        json!([{"hooks": [{"type": "command", "command": "/opt/prompt-hook"}]}, date()])
    );
    assert_eq!(got["hooks"]["Stop"], json!([stop()]));
    assert_eq!(
        got["hooks"]["SessionEnd"],
        json!([{"hooks": [{"type": "command", "command": "/opt/end"}]}])
    );
}

#[test]
fn exec_form_registrations_including_windows_paths_are_recognized() {
    let home = Home::new();
    let windows = r"C:\Users\u\.cargo\bin\claude-hooks.exe";
    home.write_settings(&json!({"hooks": {"PreToolUse": [
        {"matcher": "Bash|Agent|Skill|Edit|Write", "hooks": [
            {"type": "command", "command": windows, "args": ["pre-tool-use"], "timeout": 60}
        ]},
    ]}}));
    let got = home.ok(&json!({}), &["--pre-tool-use", windows]);
    assert_eq!(
        got["hooks"]["PreToolUse"],
        json!([{"matcher": "Bash|Agent|Skill|Edit|Write|SendMessage|SubagentHandback", "hooks": [
            {"type": "command", "command": windows, "args": ["pre-tool-use"], "timeout": 60}
        ]}])
    );
}

#[test]
fn running_twice_gives_the_same_settings() {
    let home = Home::new();
    let args = ["--pre-tool-use", HOOKS, "--stop", HOOKS];
    let first = home.ok(&json!({"language": "Japanese"}), &args);
    let second = home.ok(&json!({"language": "Japanese"}), &args);
    assert_eq!(first, second);
}

#[test]
fn hooks_that_are_not_given_are_left_as_they_are() {
    // 渡さない hook の登録には触らない
    let home = Home::new();
    let existing = json!([{"hooks": [{"type": "command", "command": "/opt/stop"}]}]);
    home.write_settings(&json!({"hooks": {"Stop": existing}}));
    let got = home.ok(&json!({}), &["--pre-tool-use", HOOKS]);
    assert_eq!(got["hooks"]["Stop"], existing);
    assert_eq!(got["hooks"]["PreToolUse"], json!([pre_tool_use()]));
}

#[test]
fn a_broken_settings_file_is_left_untouched() {
    let home = Home::new();
    std::fs::write(home.settings(), "{ not json").unwrap();
    let out = home.run(&json!({}), &["--pre-tool-use", HOOKS]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        std::fs::read_to_string(home.settings()).unwrap(),
        "{ not json"
    );
}

#[test]
fn the_key_order_of_the_file_is_kept() {
    let home = Home::new();
    std::fs::write(home.settings(), r#"{"zeta": 1, "alpha": 2}"#).unwrap();
    home.ok(&json!({}), &[]);
    let text = std::fs::read_to_string(home.settings()).unwrap();
    assert!(
        text.find("zeta").unwrap() < text.find("alpha").unwrap(),
        "{text}"
    );
}

#[test]
fn groups_without_a_hooks_list_are_kept() {
    // ハーネスの規則に当たらない組は、形が崩れていても消さない
    let home = Home::new();
    home.write_settings(&json!({"hooks": {"Stop": [{"matcher": "x"}]}}));
    let got = home.ok(&json!({}), &["--stop", HOOKS]);
    assert_eq!(got["hooks"]["Stop"], json!([{"matcher": "x"}, stop()]));
}

#[test]
fn an_event_that_is_not_a_list_is_left_untouched() {
    let home = Home::new();
    let text = r#"{"hooks": {"PreToolUse": {"bad": 1}}}"#;
    std::fs::write(home.settings(), text).unwrap();
    let out = home.run(&json!({}), &["--pre-tool-use", HOOKS]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(std::fs::read_to_string(home.settings()).unwrap(), text);
}

#[test]
fn the_old_memory_backup_registration_without_args_is_replaced() {
    // 前の退避のバイナリ claude-memory-backup の登録は、claude-hooks stop の 1 件に置き換わる
    let home = Home::new();
    let old = r"C:\Users\u\.claude\bin\claude-memory-backup.exe";
    let hooks = r"C:\Users\u\.claude\bin\claude-hooks.exe";
    home.write_settings(&json!({"hooks": {"Stop": [
        {"hooks": [{"type": "command", "command": old, "timeout": 60}]},
    ]}}));
    let got = home.ok(&json!({}), &["--stop", hooks]);
    assert_eq!(
        got["hooks"]["Stop"],
        json!([{"hooks": [{"type": "command", "command": hooks, "args": ["stop"], "timeout": 60}]}])
    );
}

#[test]
fn a_shell_form_stop_registration_of_claude_hooks_is_replaced() {
    let home = Home::new();
    home.write_settings(&json!({"hooks": {"Stop": [
        {"hooks": [{"type": "command", "command": "/home/u/.claude/bin/claude-hooks stop", "timeout": 60}]},
        {"hooks": [{"type": "command", "command": "/opt/stop"}]},
    ]}}));
    let got = home.ok(&json!({}), &["--stop", HOOKS]);
    assert_eq!(
        got["hooks"]["Stop"],
        json!([{"hooks": [{"type": "command", "command": "/opt/stop"}]}, stop()])
    );
}

#[test]
fn null_hooks_count_as_no_hooks() {
    // jq の filter は null の hooks を空のオブジェクトとして扱った
    let home = Home::new();
    home.write_settings(&json!({"hooks": null}));
    let got = home.ok(&json!({}), &["--pre-tool-use", HOOKS]);
    assert_eq!(got["hooks"]["PreToolUse"], json!([pre_tool_use()]));
}

#[test]
fn a_read_only_settings_file_is_replaced() {
    // Windows は読むだけの属性のファイルの上に名前を付け替えられない
    let home = Home::new();
    home.write_settings(&json!({}));
    let mut perms = std::fs::metadata(home.settings()).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(home.settings(), perms).unwrap();
    let got = home.ok(&json!({}), &["--pre-tool-use", HOOKS]);
    assert_eq!(got["hooks"]["PreToolUse"], json!([pre_tool_use()]));
}

#[test]
fn a_null_event_counts_as_no_registrations() {
    // jq の filter は null のイベントを空の配列として扱った
    let home = Home::new();
    home.write_settings(&json!({"hooks": {"PreToolUse": null}}));
    let got = home.ok(&json!({}), &["--pre-tool-use", HOOKS]);
    assert_eq!(got["hooks"]["PreToolUse"], json!([pre_tool_use()]));
}

#[test]
fn a_settings_file_with_a_utf8_bom_is_read() {
    let home = Home::new();
    std::fs::write(home.settings(), "\u{feff}{\"theme\": \"dark\"}").unwrap();
    let got = home.ok(&json!({"language": "Japanese"}), &[]);
    assert_eq!(got["theme"], json!("dark"));
    assert_eq!(got["language"], json!("Japanese"));
}

/// UserPromptSubmit の登録（現在時刻の `date` と、`--user-prompt-submit` のバイナリの既定ブランチの進みの知らせ。#363）
fn prompt(command: &str) -> Value {
    json!({"hooks": [{"type": "command", "command": command, "timeout": 10}]})
}

const PROMPT: &str = "date '+現在時刻: %F %H:%M %Z'; '/home/u/.claude/bin/claude-hooks' user-prompt-submit || exit 1";

#[test]
fn the_prompt_hook_runs_the_date_and_the_binary_in_one_registration() {
    let home = Home::new();
    let got = home.ok(&json!({}), &["--user-prompt-submit", HOOKS]);
    assert_eq!(got["hooks"]["UserPromptSubmit"], json!([prompt(PROMPT)]));
}

#[test]
fn the_date_only_registration_is_replaced_by_the_prompt_hook() {
    // 前の登録（現在時刻だけ）の上に流すと、新しい登録 1 件になる。もう 1 度流しても 1 件のまま
    let home = Home::new();
    let other = json!({"hooks": [{"type": "command", "command": "/opt/prompt-hook"}]});
    home.write_settings(&json!({"hooks": {"UserPromptSubmit": [date(), other]}}));
    let args = [
        "--pre-tool-use",
        HOOKS,
        "--stop",
        HOOKS,
        "--user-prompt-submit",
        HOOKS,
    ];
    let got = home.ok(&json!({}), &args);
    assert_eq!(
        got["hooks"]["UserPromptSubmit"],
        json!([other, prompt(PROMPT)])
    );
    let again = home.ok(&json!({}), &args);
    assert_eq!(again, got);
}

#[test]
fn the_prompt_hook_quotes_a_windows_path_for_git_bash() {
    // Windows の shell form は Git Bash で走る。`\` を残すよう単一引用符で括り、パスの中の `'` は閉じて書き直す
    let home = Home::new();
    let windows = r"C:\Users\o'neil\.claude\bin\claude-hooks.exe";
    let got = home.ok(&json!({}), &["--user-prompt-submit", windows]);
    assert_eq!(
        got["hooks"]["UserPromptSubmit"],
        json!([prompt(
            r"date '+現在時刻: %F %H:%M %Z'; 'C:\Users\o'\''neil\.claude\bin\claude-hooks.exe' user-prompt-submit || exit 1"
        )])
    );
}

/// 登録したコマンドを、Claude Code と同じく sh -c で、標準入力に hook の入力を渡して実行する
#[cfg(unix)]
fn run_prompt_hook(home: &Home, bin: &Path) -> Output {
    use std::io::Write;
    let got = home.ok(&json!({}), &["--user-prompt-submit", bin.to_str().unwrap()]);
    let command = got["hooks"]["UserPromptSubmit"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .to_string();
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(&command)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"session_id":"s","cwd":"/"}"#)
        .unwrap();
    child.wait_with_output().unwrap()
}

/// body を本文にした偽のバイナリ（空白を含むパスに置き、引用を確かめる）
#[cfg(unix)]
fn fake_binary(home: &Home, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let bin = home.0.join("bin dir/claude-hooks");
    std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
    std::fs::write(&bin, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    // 並行するテストが書き込みの間に fork した子は、exec まで書き込みに開いた fd を持ち、その間の起動は ETXTBSY で
    // 失敗する。起動できるまで待つ（本文は引数の probe では何もしないか、2 で終わるだけ）
    for _ in 0..200 {
        match Command::new(&bin)
            .arg("probe")
            .stdin(std::process::Stdio::null())
            .output()
        {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
            _ => return bin,
        }
    }
    panic!("{} が 2 秒たっても起動できない（ETXTBSY）", bin.display());
}

#[cfg(unix)]
#[test]
fn the_registered_prompt_hook_prints_the_date_line_then_the_binary_output() {
    let home = Home::new();
    // バイナリは hook の入力を標準入力で受ける（date は読まない）
    let bin = fake_binary(
        &home,
        r#"test "$1" = user-prompt-submit && echo "got: $(cat)""#,
    );
    let out = run_prompt_hook(&home, &bin);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "{stdout}");
    assert!(lines[0].starts_with("現在時刻: "), "{stdout}");
    assert_eq!(lines[1], r#"got: {"session_id":"s","cwd":"/"}"#);
}

#[cfg(unix)]
#[test]
fn a_failing_binary_does_not_block_the_prompt() {
    // UserPromptSubmit の終了コード 2 はプロンプトを止めて消す。前の版のバイナリは知らないサブコマンドに 2 で終わる
    let home = Home::new();
    let bin = fake_binary(&home, "echo usage >&2; exit 2");
    let out = run_prompt_hook(&home, &bin);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .starts_with("現在時刻: ")
    );
}
