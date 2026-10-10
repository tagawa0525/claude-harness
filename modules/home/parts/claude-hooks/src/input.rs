//! PreToolUse hook の入力 JSON

use std::path::PathBuf;

pub struct Input {
    pub tool_name: String,
    /// セッションをまたがない記録（読み込んだスキル）の鍵。無ければ空
    pub session_id: String,
    pub command: String,
    /// Write / Edit の対象ファイル
    pub file_path: String,
    /// Skill ツールで読み込むスキルの名前
    pub skill: String,
    /// UserPromptSubmit の入力（ユーザーが打った文）
    pub prompt: String,
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
            session_id: s("/session_id").unwrap_or_default(),
            command: s("/tool_input/command").unwrap_or_default(),
            file_path: s("/tool_input/file_path").unwrap_or_default(),
            skill: s("/tool_input/skill").unwrap_or_default(),
            prompt: s("/prompt").unwrap_or_default(),
            run_in_background: v
                .pointer("/tool_input/run_in_background")
                .and_then(|x| x.as_bool())
                .unwrap_or(false),
            cwd,
        })
    }
}
