//! コミットのメッセージの規約（ADR-0014、ADR-0058、ADR-0073、ADR-0085）。
//!
//! - 件名は `#` で始まらない最初の行。Merge、fixup!、squash!、Revert で始まる件名は、元のコミットからコピーした
//!   ものか git が作るもの（fixup と squash の本文は畳まれて履歴に残る）なので、形と長さを見ず、語の検査からも除く
//! - 語を検査する本文は、commit -v の scissors の行から後（差分）を除いたもの。`#` で始まる行は、-m や -F
//!   （cleanup=whitespace）では消えずにコミットに残るので、除かない
//! - 本文は Markdown として読む（文書の検査と同じく、コードスパンの中は見ない。ADR-0058）

use regress::Regex;

/// Conventional Commits の type
const TYPES: &str = "feat|fix|docs|style|refactor|test|chore|perf|build|ci|revert";
/// 件名の上限と、超えたら警告する長さ（文字の数）
const LIMIT: usize = 72;
const WARN: usize = 50;

/// commit -v の scissors の行
const SCISSORS: &str = "# ------------------------ >8 ------------------------";

/// メッセージの件名（`#` で始まらない最初の行。無ければ空）
pub fn subject(message: &str) -> &str {
    message
        .split('\n')
        .find(|line| !line.starts_with('#'))
        .unwrap_or("")
}

/// 形と長さを見ず、語の検査からも除く件名か
pub fn is_exempt(subject: &str) -> bool {
    ["Merge", "fixup!", "squash!", "Revert"]
        .iter()
        .any(|p| subject.starts_with(p))
}

/// 末尾の改行を除く（sh のコマンド置換と同じ）
fn trim_newlines(text: &str) -> &str {
    text.trim_end_matches('\n')
}

/// 語を検査する本文（Markdown の文として、末尾に改行を 1 つ付けたもの）。空白だけなら None
pub fn body(message: &str) -> Option<String> {
    // sh の変数は NUL を持てない
    let message = message.replace('\0', "");
    let mut lines: Vec<&str> = trim_newlines(&message).split('\n').collect();
    if is_exempt(subject(&message)) {
        // 件名は空でも # でもない最初の行
        if let Some(at) = lines
            .iter()
            .position(|l| !l.is_empty() && !l.starts_with('#'))
        {
            lines.remove(at);
        }
    }
    let end = lines
        .iter()
        .position(|l| *l == SCISSORS)
        .unwrap_or(lines.len());
    let body = trim_newlines(&lines[..end].join("\n")).to_string();
    body.chars()
        .any(|c| !c.is_whitespace())
        .then(|| body + "\n")
}

/// 件名の形と長さの判定
#[derive(Debug, PartialEq, Eq)]
pub enum Subject {
    /// 通す。長さの警告があればその文
    Pass(Option<String>),
    /// 止める理由
    Reject(String),
}

/// 件名の形（Conventional Commits）と長さを見る。is_exempt の件名には呼ばない
pub fn check_subject(subject: &str) -> Subject {
    let form = Regex::new(&format!("^({TYPES})(\\([^)]+\\))?!?: [^ ]")).expect("固定の正規表現");
    if form.find(subject).is_none() {
        return Subject::Reject(format!(
            "❌ Conventional Commits 形式ではありません: {subject}\n   形式: <type>(<scope>)?: <subject>    type: {TYPES}"
        ));
    }
    let length = subject.chars().count();
    if length > LIMIT {
        return Subject::Reject(format!(
            "❌ 件名が {LIMIT} 文字を超えています ({length} 文字): {subject}"
        ));
    }
    Subject::Pass((length > WARN).then(|| {
        format!(
            "⚠️  件名が {WARN} 文字を超えています ({length} 文字)。短くできないか検討してください"
        )
    }))
}
