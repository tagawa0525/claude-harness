//! require-adr-skill: ADR（docs/adr/）を書く前に /adr スキルを読ませる
//!
//! 手元の ADR を手本に形を真似ると、欠陥ごと複製される（ADR-0005）。書く時点でスキルの本文
//! （一般的な形）が読み込まれていることを確かめる。読み込みはモデルが付ける印ではなく、
//! Skill ツールの呼び出しをこの hook 自身が記録する（セッション単位）。
//! 他人のプロジェクトでは、そのプロジェクトの流儀に従うので外す（git::own_project）

use super::Rule;
use crate::git;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;
use std::path::{Path, PathBuf};

pub struct RequireAdrSkill;

const SKILL: &str = "adr";

impl Rule for RequireAdrSkill {
    fn name(&self) -> &'static str {
        "require-adr-skill"
    }

    fn tools(&self) -> &'static [&'static str] {
        &["Skill", "Write", "Edit", "MultiEdit"]
    }

    fn check(&self, input: &Input, _shell: &Shell) -> Vec<Finding> {
        // セッションが分からなければ読み込みを確かめられない。止めずに通す
        if input.session_id.is_empty() {
            return Vec::new();
        }
        if input.tool_name == "Skill" {
            if input.skill == SKILL || input.skill.ends_with(&format!(":{SKILL}")) {
                record(&input.session_id, SKILL);
            }
            return Vec::new();
        }
        let path = input.cwd.join(&input.file_path);
        let Some((root, rel)) = repo_relative(&path) else {
            return Vec::new();
        };
        if !is_adr(&rel) || !git::own_project(&root) || loaded(&input.session_id, SKILL) {
            return Vec::new();
        }
        vec![Finding::Deny(format!(
            "ADR（{}）を書く前に /adr スキルを読んでください（Skill ツールで adr）。既存の ADR を形の手本にせず、スキルの一般的な形で書きます。他人のプロジェクトでそのプロジェクトの流儀に従うなら、git config claude-hooks.own-project false で外せます",
            rel.display()
        ))]
    }
}

/// docs/adr/ 配下の Markdown か（リポジトリのルートからの相対パス）
fn is_adr(rel: &Path) -> bool {
    rel.starts_with("docs/adr") && rel.extension().is_some_and(|e| e == "md")
}

/// ファイルのリポジトリのルートと、ルートからの相対パス。ファイルや親ディレクトリが
/// まだ無くても、存在する最も近い祖先から求める
fn repo_relative(path: &Path) -> Option<(PathBuf, PathBuf)> {
    let mut existing = path.parent()?;
    while !existing.exists() {
        existing = existing.parent()?;
    }
    let root = PathBuf::from(git::git(existing, &["rev-parse", "--show-toplevel"])?);
    // git はシンボリックリンクを解決したパスを返すので、こちらも解決してから比べる
    let rest = path.strip_prefix(existing).ok()?;
    let full = existing.canonicalize().ok()?.join(rest);
    let rel = full
        .strip_prefix(root.canonicalize().ok()?)
        .ok()?
        .to_path_buf();
    Some((root, rel))
}

/// セッションで読み込んだスキルの記録。XDG_RUNTIME_DIR（再起動で消える）に置く
fn record_file(session_id: &str) -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let safe: String = session_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    base.join("claude-hooks").join("skills").join(safe)
}

fn record(session_id: &str, skill: &str) {
    use std::io::Write;
    let file = record_file(session_id);
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
    {
        let _ = writeln!(f, "{skill}");
    }
}

fn loaded(session_id: &str, skill: &str) -> bool {
    std::fs::read_to_string(record_file(session_id)).is_ok_and(|s| s.lines().any(|l| l == skill))
}
