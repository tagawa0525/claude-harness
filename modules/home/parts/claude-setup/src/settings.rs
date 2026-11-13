//! `claude-setup settings --static <JSON> [--pre-tool-use <バイナリ>] [--stop <バイナリ>] [--legacy <ファイル名>]...`:
//! ~/.claude/settings.json に、ハーネスの静的な設定を重ね、ハーネスの hook の登録を 1 件ずつに揃える（ADR-0084）。
//!
//! - 静的な設定: JSON のファイルのいちばん上のキーごとに置き換える（ほかのキーとその順は残す）
//! - PreToolUse: `--pre-tool-use` のバイナリを、Bash、Agent、Skill の組に exec form で 1 件だけ登録する。前の
//!   claude-hooks の登録（shell form と exec form。Windows の `\` 区切りのパスも）と、`--legacy` の名前の旧 bash hook の
//!   登録を外し、ほかの hook は残す。バイナリだけの組にするのは、ほかの hook と同じ組に置くと、その組の matcher を
//!   変えたときにほかの hook も Agent で走るため
//! - Stop: `--stop` のバイナリ（自動メモリの退避）を 1 件だけ登録する。async にしないのは、async の hook の失敗の文が
//!   利用者に見えず（Claude にだけ次のターンで渡る）、退避の失敗に気づけないため
//! - UserPromptSubmit: 現在時刻を 1 行出すコマンドを 1 件だけ登録する（どちらの OS も同じ文字列の shell form）
//! - 渡さない hook の登録には触らない。読めないか壊れた settings.json は書き換えずに止める

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

/// claude-hooks の matcher
const PRE_TOOL_USE_MATCHER: &str = "Bash|Agent|Skill";
/// hook の timeout は秒（https://code.claude.com/docs/en/hooks の "Seconds before canceling."。command の既定は 600）。
/// pre-merge-check の gh は最大 14 か所の呼び出しで、1 回 0.4〜0.7 秒（2026-10-28 に gh pr view、repo view、api を
/// 手元で測定）なので順に呼んでも約 10 秒。その数倍の 60 秒にする。hook が止まったとき、Agent の起動もこの時間待つ
const PRE_TOOL_USE_TIMEOUT: u64 = 60;
/// git の操作は実際のメモリ（2026-11-06 に 17 プロジェクト、130 ファイル、624 KB）のコピーで 1 回 0.02 秒以下。待つのは
/// ほかの応答の hook が持つロック（最大 30 秒）なので、それより長くする
const STOP_TIMEOUT: u64 = 60;
/// 報告とメモリに書く時刻を推測しないため、ターンごとに現在時刻を 1 行、文脈に入れる（標準出力が文脈に足される）
const PROMPT_DATE: &str = "date '+現在時刻: %F %H:%M %Z'";
/// 現在時刻の登録を見分ける先頭（コマンドを変えても古い登録を消せるように）
const PROMPT_DATE_PREFIX: &str = "date '+現在時刻";

/// 登録するもの
pub struct Options {
    pub statics: PathBuf,
    pub pre_tool_use: Option<String>,
    pub stop: Option<String>,
    pub legacy: Vec<String>,
}

pub fn run(home: &Path, options: &Options) -> Result<(), String> {
    let statics: Value = read_json(&options.statics)?;
    let Value::Object(statics) = statics else {
        return Err(format!(
            "❌ {} はオブジェクトの JSON ではありません",
            options.statics.display()
        ));
    };
    let path = home.join(".claude/settings.json");
    let mut settings = if path.exists() {
        match read_json(&path)? {
            Value::Object(map) => map,
            _ => {
                return Err(format!(
                    "❌ {} はオブジェクトの JSON ではありません。書き換えずに止めます",
                    path.display()
                ));
            }
        }
    } else {
        Map::new()
    };
    for (key, value) in statics {
        settings.insert(key, value);
    }
    let hooks = settings
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    // jq の filter と同じく、null は登録の無い hooks として扱う
    if hooks.is_null() {
        *hooks = Value::Object(Map::new());
    }
    let Value::Object(hooks) = hooks else {
        return Err(format!(
            "❌ {} の hooks がオブジェクトではありません。書き換えずに止めます",
            path.display()
        ));
    };
    if let Some(bin) = &options.pre_tool_use {
        let entry = json!({
            "matcher": PRE_TOOL_USE_MATCHER,
            "hooks": [{"type": "command", "command": bin, "args": ["pre-tool-use"], "timeout": PRE_TOOL_USE_TIMEOUT}],
        });
        replace(&path, hooks, "PreToolUse", entry, |hook| {
            is_binary(hook, "claude-hooks", Some("pre-tool-use"))
                || options
                    .legacy
                    .iter()
                    .any(|name| command(hook).is_some_and(|c| c.ends_with(&format!("/{name}"))))
        })?;
    }
    replace(
        &path,
        hooks,
        "UserPromptSubmit",
        json!({"hooks": [{"type": "command", "command": PROMPT_DATE}]}),
        |hook| command(hook).is_some_and(|c| c.starts_with(PROMPT_DATE_PREFIX)),
    )?;
    if let Some(bin) = &options.stop {
        let entry = json!({
            "hooks": [{"type": "command", "command": bin, "args": [], "timeout": STOP_TIMEOUT}],
        });
        replace(&path, hooks, "Stop", entry, |hook| {
            is_binary(hook, "claude-memory-backup", None)
        })?;
    }
    write_json(&path, &Value::Object(settings))
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("❌ {} を読めません（{e}）", path.display()))?;
    serde_json::from_str(&text).map_err(|e| {
        format!(
            "❌ {} を JSON として読めません（{e}）。書き換えずに止めます",
            path.display()
        )
    })
}

/// 隣の一時のファイルに書いてから名前を付け替える（Claude Code が読む途中のファイルを見せない）
fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).expect("JSON の値は書ける") + "\n";
    crate::sync::replace_with(path, |tmp| fs::write(tmp, text))
        .map_err(|e| format!("❌ {} を書けません（{e}）", path.display()))
}

fn command(hook: &Value) -> Option<&str> {
    hook.get("command").and_then(Value::as_str)
}

/// ハーネスのバイナリ name の登録か。shell form は `command` の末尾が `/bin/<name>`（と ` <arg>`）、exec form は
/// `command` のファイル名が name（Windows は name.exe。`\` 区切りも）で `args` が [arg]（arg が無ければ空か無し）
fn is_binary(hook: &Value, name: &str, arg: Option<&str>) -> bool {
    let Some(command) = command(hook) else {
        return false;
    };
    let shell = match arg {
        Some(arg) => format!("/bin/{name} {arg}"),
        None => format!("/bin/{name}"),
    };
    if command.ends_with(&shell) {
        return true;
    }
    let file = command.rsplit(['/', '\\']).next().unwrap_or(command);
    let file = file.strip_suffix(".exe").unwrap_or(file);
    let want: Vec<Value> = arg.map(|a| vec![json!(a)]).unwrap_or_default();
    let args = match hook.get("args") {
        None => Some(&[][..]),
        Some(args) => args.as_array().map(Vec::as_slice),
    };
    file == name && args == Some(want.as_slice())
}

/// event の登録から remove に当たる hook を外し、空になった組を消して、entry を足す（path は失敗の文に出す
/// settings.json）
fn replace(
    path: &Path,
    hooks: &mut Map<String, Value>,
    event: &str,
    entry: Value,
    remove: impl Fn(&Value) -> bool,
) -> Result<(), String> {
    let groups = hooks
        .entry(event)
        .or_insert_with(|| Value::Array(Vec::new()));
    // jq の filter と同じく、null は登録の無いイベントとして扱う
    if groups.is_null() {
        *groups = Value::Array(Vec::new());
    }
    let Value::Array(groups) = groups else {
        return Err(format!(
            "❌ {} の hooks.{event} が配列ではありません。書き換えずに止めます",
            path.display()
        ));
    };
    for group in groups.iter_mut() {
        if let Some(list) = group.get_mut("hooks").and_then(Value::as_array_mut) {
            list.retain(|hook| !remove(hook));
        }
    }
    // hooks の配列を持たない組はハーネスの規則に当たらないので残し、外して空になった組だけを消す
    groups.retain(|group| match group.get("hooks") {
        Some(Value::Array(list)) => !list.is_empty(),
        _ => true,
    });
    groups.push(entry);
    Ok(())
}
