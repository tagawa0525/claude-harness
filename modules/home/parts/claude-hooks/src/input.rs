//! PreToolUse hook の入力 JSON

use std::path::PathBuf;

pub struct Input {
    pub tool_name: String,
    pub command: String,
    pub run_in_background: bool,
    /// Agent ツールの tool_input.prompt（Bash では空）
    pub prompt: String,
    /// Skill ツールの tool_input.skill（スキルの名前。ほかのツールでは空）
    pub skill: String,
    /// Skill ツールの tool_input.args（スキルに渡す引数の文字列。無ければ空）
    pub args: String,
    /// hook 実行時のカレントディレクトリ。JSON の cwd が無ければプロセスの cwd
    pub cwd: PathBuf,
    /// Claude Code のセッションの ID。scratchpad のパス（`…/<session_id>/scratchpad`）を見分けるのに使う。無ければ空
    pub session_id: String,
}

impl Input {
    pub fn parse(raw: &str) -> Option<Input> {
        let v: serde_json::Value = serde_json::from_str(raw).ok()?;
        let s = |ptr: &str| v.pointer(ptr).and_then(|x| x.as_str()).map(str::to_string);
        let cwd = s("/cwd")
            .filter(|c| !c.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())?;
        Some(Input {
            tool_name: s("/tool_name").unwrap_or_default(),
            command: s("/tool_input/command").unwrap_or_default(),
            run_in_background: v
                .pointer("/tool_input/run_in_background")
                .and_then(|x| x.as_bool())
                .unwrap_or(false),
            prompt: s("/tool_input/prompt").unwrap_or_default(),
            skill: s("/tool_input/skill").unwrap_or_default(),
            args: s("/tool_input/args").unwrap_or_default(),
            cwd,
            session_id: s("/session_id").unwrap_or_default(),
        })
    }
}
