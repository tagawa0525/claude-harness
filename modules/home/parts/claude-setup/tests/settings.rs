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
const BACKUP: &str = "/home/u/.claude/bin/claude-memory-backup";
const DATE: &str = "date '+現在時刻: %F %H:%M %Z'";

fn pre_tool_use() -> Value {
    json!({"matcher": "Bash|Agent|Skill", "hooks": [
        {"type": "command", "command": HOOKS, "args": ["pre-tool-use"], "timeout": 60}
    ]})
}

fn stop() -> Value {
    json!({"hooks": [{"type": "command", "command": BACKUP, "args": [], "timeout": 60}]})
}

fn date() -> Value {
    json!({"hooks": [{"type": "command", "command": DATE}]})
}

#[test]
fn a_missing_settings_file_gets_the_static_settings_and_the_hooks() {
    let home = Home::new();
    let got = home.ok(
        &json!({"language": "Japanese"}),
        &["--pre-tool-use", HOOKS, "--stop", BACKUP],
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
            {"matcher": "Bash|Agent|Skill", "hooks": [
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
            BACKUP,
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
        {"matcher": "Bash|Agent|Skill", "hooks": [
            {"type": "command", "command": windows, "args": ["pre-tool-use"], "timeout": 60}
        ]},
    ]}}));
    let got = home.ok(&json!({}), &["--pre-tool-use", windows]);
    assert_eq!(
        got["hooks"]["PreToolUse"],
        json!([{"matcher": "Bash|Agent|Skill", "hooks": [
            {"type": "command", "command": windows, "args": ["pre-tool-use"], "timeout": 60}
        ]}])
    );
}

#[test]
fn running_twice_gives_the_same_settings() {
    let home = Home::new();
    let args = ["--pre-tool-use", HOOKS, "--stop", BACKUP];
    let first = home.ok(&json!({"language": "Japanese"}), &args);
    let second = home.ok(&json!({"language": "Japanese"}), &args);
    assert_eq!(first, second);
}

#[test]
fn hooks_that_are_not_given_are_left_as_they_are() {
    // Windows は claude-memory-backup を入れない（#101 の段階 8 まで）。渡さない hook の登録には触らない
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
    let got = home.ok(&json!({}), &["--stop", BACKUP]);
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
fn an_exec_form_registration_without_args_is_recognized() {
    let home = Home::new();
    let windows = r"C:\Users\u\.claude\bin\claude-memory-backup.exe";
    home.write_settings(&json!({"hooks": {"Stop": [
        {"hooks": [{"type": "command", "command": windows, "timeout": 60}]},
    ]}}));
    let got = home.ok(&json!({}), &["--stop", windows]);
    assert_eq!(
        got["hooks"]["Stop"],
        json!([{"hooks": [{"type": "command", "command": windows, "args": [], "timeout": 60}]}])
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
