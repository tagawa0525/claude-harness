//! PreToolUse hook の入力 JSON

use std::path::PathBuf;

pub struct Input {
    pub tool_name: String,
    pub command: String,
    pub run_in_background: bool,
    /// hook 実行時のカレントディレクトリ。JSON の cwd が無ければプロセスの cwd
    pub cwd: PathBuf,
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
            cwd,
        })
    }
}
