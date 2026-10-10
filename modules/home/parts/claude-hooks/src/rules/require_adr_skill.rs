//! require-adr-skill: ADR（docs/adr/）を書く前に /adr スキルを読ませる
//!
//! 手元の ADR を手本に形を真似ると、欠陥ごと複製される（ADR-0005）。書く時点でスキルの本文
//! （一般的な形）が読み込まれていることを確かめる。読み込みはモデルが付ける印ではなく、
//! Skill ツールの成功（PostToolUse、`claude-hooks post-tool-use`）をこの hook 自身が記録する
//! （セッション単位）。呼び出し前（PreToolUse）に記録しないのは、拒否や展開の失敗で本文が
//! 読み込まれないことがあるため。Bash での書き込みは見ないので、コミット時の節の検査
//! （crate::adr）が受け持つ。他人のプロジェクトでは、そのプロジェクトの流儀に従うので外す（git::own_project）

use super::Rule;
use crate::adr::is_adr;
use crate::git;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::Shell;
use std::path::{Component, Path, PathBuf};

pub struct RequireAdrSkill;

const SKILL: &str = "adr";

impl Rule for RequireAdrSkill {
    fn name(&self) -> &'static str {
        "require-adr-skill"
    }

    fn tools(&self) -> &'static [&'static str] {
        &["Write", "Edit", "MultiEdit"]
    }

    fn check(&self, input: &Input, _shell: &Shell) -> Vec<Finding> {
        // セッションが分からなければ読み込みを確かめられない。止めずに通す
        if input.session_id.is_empty() {
            return Vec::new();
        }
        let path = normalize(&input.cwd.join(&input.file_path));
        // ADR 以外の編集で git を呼ばないよう、パスの形を先に見る
        if !path.parent().is_some_and(|d| d.ends_with("docs/adr")) {
            return Vec::new();
        }
        let Some((root, rel)) = repo_relative(&path) else {
            return Vec::new();
        };
        if !is_adr(&rel.to_string_lossy())
            || loaded(&input.session_id, SKILL)
            || !git::own_project(&root)
        {
            return Vec::new();
        }
        vec![Finding::Deny(format!(
            "ADR（{}）を書く前に /adr スキルを読んでください（Skill ツールで adr）。既存の ADR を形の手本にせず、スキルの一般的な形で書きます。他人のプロジェクトでそのプロジェクトの流儀に従うなら、git config claude-hooks.own-project false で外せます",
            rel.display()
        ))]
    }
}

/// Skill ツールが成功したとき（PostToolUse）に、読み込んだスキルを記録する。
/// 名前は `adr`、`/adr`、プラグインの `<plugin>:adr` のどれでもよい
pub fn record_skill(input: &Input) {
    let name = input.skill.trim_start_matches('/');
    if input.tool_name == "Skill"
        && !input.session_id.is_empty()
        && (name == SKILL || name.ends_with(&format!(":{SKILL}")))
    {
        record(&input.session_id, SKILL);
    }
}

/// ユーザーが `/adr` と打ったとき（UserPromptSubmit）に記録する。スラッシュコマンドは
/// Skill ツールを通らずに展開されるので、PostToolUse では捉えられない
pub fn record_prompt(input: &Input) {
    let Some(rest) = input.prompt.trim_start().strip_prefix(&format!("/{SKILL}")) else {
        return;
    };
    if !input.session_id.is_empty() && rest.chars().next().is_none_or(char::is_whitespace) {
        record(&input.session_id, SKILL);
    }
}

/// 会話を要約する前（PreCompact）に記録を消す。要約でスキルの本文が文脈から消えるので、
/// 要約の後に ADR を書くときは読み込み直させる
pub fn forget(input: &Input) {
    if let Some(file) = record_file(&input.session_id) {
        let _ = std::fs::remove_file(file);
    }
}

/// `.` と `..` を字面で畳む。途中のディレクトリがまだ無くても、書き込み先と同じ場所を指す
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
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

/// セッションで読み込んだスキルの記録。XDG_RUNTIME_DIR（再起動で消える）に置き、無ければ
/// XDG_CACHE_HOME か ~/.cache に置く。共有の /tmp は、ほかのユーザーが記録を作れてしまうので使わない
fn record_file(session_id: &str) -> Option<PathBuf> {
    if session_id.is_empty() {
        return None;
    }
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .or_else(|| std::env::var_os("XDG_CACHE_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
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
    Some(base.join("claude-hooks").join("skills").join(safe))
}

fn record(session_id: &str, skill: &str) {
    use std::io::Write;
    let Some(file) = record_file(session_id) else {
        return;
    };
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
    record_file(session_id)
        .and_then(|f| std::fs::read_to_string(f).ok())
        .is_some_and(|s| s.lines().any(|l| l == skill))
}
