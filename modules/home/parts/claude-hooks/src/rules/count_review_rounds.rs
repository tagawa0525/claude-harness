//! count-review-rounds: ローカルの /code-review の周をブランチごとに数え、何周目かをモデルに伝える（止めない）
//!
//! 1 周目の後にもう 1 周回すかは、直しの大きさと危険からモデルが決める（/git-merge の「直す指摘の基準」）。数えるのは
//! 決定的なので hook が行い、判断の材料として周の数を渡す。周の数で止めないのは、確かめた誤りの直しまで数で止めると、
//! 直しが誰にも読まれずに残るか、別の名前のブランチに移す回り道を誘うため。
//!
//! - 数える呼び出し: Skill の `code-review` だけ。対象は引数の中で最初にある、ディレクトリとして在る絶対パス（無ければ
//!   hook の cwd）で、その worktree がチェックアウトしているブランチを数える。親が Agent で担当にレビューを頼む周も、
//!   担当が呼ぶ Skill で数える。Agent の prompt は見ない（自然文から「/code-review を頼む」を見分ける判定は、
//!   否定や別のスキルを呼ぶ文に当たるか、頼む文を落とした）
//! - 置き場: リポジトリの共通の git ディレクトリの `claude-review-rounds/<ブランチ名>`（`/` は `%2F`）。1 行が 1 周で、
//!   回したときの HEAD と時刻を書く。worktree を消して作り直しても、rebase で HEAD が変わっても数は残る。ブランチの
//!   reflog の最も古い時刻より前の行は、消した前の同じ名前のブランチのものとして数えない（ブランチを消すと reflog も
//!   消え、作り直すと新しく始まる。rebase では続く。マージの前に rebase した HEAD は既定ブランチの祖先にならないので、
//!   祖先かでは見分けられない）
//! - 呼び出しの結果は見ないので、対象を見つけられなかった回とエラーの回も数える

use super::Rule;
use crate::git;
use crate::input::Input;
use crate::output::{Finding, notice};
use crate::shell::Shell;
use std::path::{Path, PathBuf};

pub struct CountReviewRounds;

/// 数を置く、共通の git ディレクトリの下のディレクトリ
const STATE_DIR: &str = "claude-review-rounds";

/// 文字列の中で最初の、ディレクトリとして在る絶対パス。囲みの `` ` ``、引用符、括弧、和文の句読点で切る
fn first_directory(text: &str) -> Option<PathBuf> {
    let stop = |c: char| c.is_whitespace() || "`'\"「」（）()。、,;".contains(c);
    text.split(stop)
        .filter(|w| w.starts_with('/'))
        .map(PathBuf::from)
        .find(|p| p.is_dir())
}

/// ブランチ名をファイル名にする（`%` と `/` を符号にする）
fn state_file_name(branch: &str) -> String {
    branch.replace('%', "%25").replace('/', "%2F")
}

/// ブランチの reflog の最も古い記録の時刻（UNIX 秒）。reflog が無ければ None
fn branch_created(dir: &Path, branch: &str) -> Option<u64> {
    let refname = format!("refs/heads/{branch}");
    let out = git::git(
        dir,
        &["reflog", "show", "--date=unix", "--format=%gd", &refname],
    )?;
    // 行は `refs/heads/<branch>@{<UNIX 秒>}`。最後の行が最も古い
    let last = out.lines().last()?;
    last.rsplit_once("@{")?.1.strip_suffix('}')?.parse().ok()
}

/// 数の置き場の行のうち、今のブランチのもの（ブランチの reflog の最も古い時刻より前の行は、消した前の同じ名前の
/// ブランチのものとして除く。reflog が無ければ全部を数える）
fn live_rounds(dir: &Path, branch: &str, path: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let created = branch_created(dir, branch);
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter(|l| {
            let at: Option<u64> = l.split_whitespace().nth(1).and_then(|t| t.parse().ok());
            match (created, at) {
                (Some(c), Some(t)) => t >= c,
                _ => true,
            }
        })
        .map(str::to_string)
        .collect()
}

fn context(branch: &str, round: usize) -> String {
    format!(
        "ブランチ {branch} のローカルの /code-review の {round} 周目です（担当と親の周の合計。rebase の後の周も数えます）。\
         周の数では止めません。直した後にもう 1 周回すかは、直しの大きさと危険で決めます（/git-merge の「直す指摘の基準」の「終わり」の項）。"
    )
}

impl Rule for CountReviewRounds {
    fn name(&self) -> &'static str {
        "count-review-rounds"
    }

    fn tools(&self) -> &'static [&'static str] {
        &["Skill"]
    }

    fn check(&self, input: &Input, _shell: &Shell) -> Vec<Finding> {
        if input.tool_name != "Skill" || input.skill != "code-review" {
            return Vec::new();
        }
        // 対象を書かない /code-review は cwd の差分を見る
        let dir = first_directory(&input.args).unwrap_or_else(|| input.cwd.clone());
        let branch = git::current_branch(&dir);
        if branch.is_empty() {
            return Vec::new();
        }
        let Some(common) = git::git(
            &dir,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        ) else {
            return Vec::new();
        };
        let state_dir = Path::new(&common).join(STATE_DIR);
        let path = state_dir.join(state_file_name(&branch));
        let mut lines = live_rounds(&dir, &branch, &path);
        let head = git::git(&dir, &["rev-parse", "HEAD"]).unwrap_or_default();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        lines.push(format!("{head} {now}"));
        let written = std::fs::create_dir_all(&state_dir)
            .and_then(|()| std::fs::write(&path, lines.join("\n") + "\n"));
        if let Err(e) = written {
            notice(format!(
                "count-review-rounds: {} に周の数を書けません: {e}",
                path.display()
            ));
            return Vec::new();
        }
        vec![Finding::Context(context(&branch, lines.len()))]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// テストの間だけある一時のディレクトリ。テストは並行に走るので、呼ぶたびに別のディレクトリにする
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> TempDir {
            static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!(
                "claude-hooks-review-rounds-{tag}-{}-{seq}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn git(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    fn commit(dir: &Path, file: &str) {
        std::fs::write(dir.join(file), file).unwrap();
        git(dir, &["add", file]);
        git(dir, &["commit", "-q", "-m", file]);
    }

    /// main に 1 コミット、`branch` に 1 コミットのリポジトリ（`branch` をチェックアウトしたまま）
    struct Repo {
        _tmp: TempDir,
        dir: PathBuf,
    }

    fn repo(branch: &str) -> Repo {
        let tmp = TempDir::new("repo");
        let dir = tmp.0.join("app");
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        // 手元のグローバルの hook（コミットの件名の検査）を使わない
        git(&dir, &["config", "core.hooksPath", "/nonexistent/hooks"]);
        commit(&dir, "init");
        git(&dir, &["switch", "-q", "-c", branch]);
        commit(&dir, "x");
        Repo { dir, _tmp: tmp }
    }

    fn skill(name: &str, args: &str, cwd: &Path) -> Input {
        let json = serde_json::json!({
            "tool_name": "Skill",
            "tool_input": {"skill": name, "args": args},
            "cwd": cwd,
        });
        Input::parse(&json.to_string()).unwrap()
    }

    fn agent(prompt: &str, cwd: &Path) -> Input {
        let json = serde_json::json!({
            "tool_name": "Agent",
            "tool_input": {"prompt": prompt, "description": "d"},
            "cwd": cwd,
        });
        Input::parse(&json.to_string()).unwrap()
    }

    fn review(r: &Repo) -> Input {
        skill(
            "code-review",
            &format!("high {}", r.dir.display()),
            Path::new("/"),
        )
    }

    fn run(input: &Input) -> Vec<Finding> {
        CountReviewRounds.check(input, &Shell::parse(""))
    }

    /// 周を回し、hook が伝えた文（止めたら panic）
    fn round(input: &Input) -> String {
        let mut context = None;
        for f in run(input) {
            match f {
                Finding::Deny(s) => panic!("周の数では止めないはず: {s}"),
                Finding::Context(s) => context = Some(s),
            }
        }
        context.expect("周の数を伝えるはず")
    }

    #[test]
    fn rounds_are_never_denied_and_each_tells_its_number() {
        let r = repo("feat/x");
        for n in 1..=9 {
            let context = round(&review(&r));
            assert!(context.contains(&format!("{n} 周目")), "{context}");
            assert!(context.contains("feat/x"), "{context}");
        }
    }

    #[test]
    fn agent_is_not_counted() {
        let r = repo("docs/y");
        let wt = r.dir.display();
        let prompts = [
            format!("/code-review を Skill で呼び、次の引数で回す: `high {wt}`。"),
            format!("`/code-review high {wt}` を Skill で呼ぶ。"),
        ];
        for prompt in &prompts {
            assert!(
                run(&agent(prompt, Path::new("/"))).is_empty(),
                "数えないはず: {prompt}"
            );
        }
        assert!(round(&review(&r)).contains("1 周目"));
    }

    #[test]
    fn other_skills_are_not_counted() {
        let r = repo("docs/y");
        let simplify = skill("simplify", &r.dir.display().to_string(), Path::new("/"));
        for _ in 0..3 {
            assert!(run(&simplify).is_empty());
        }
        assert!(round(&review(&r)).contains("1 周目"));
    }

    #[test]
    fn review_without_a_path_counts_the_branch_of_cwd() {
        let r = repo("docs/y");
        round(&skill("code-review", "high", &r.dir));
        assert!(round(&review(&r)).contains("2 周目"));
    }

    #[test]
    fn path_in_backquotes_and_followed_by_punctuation_is_found() {
        let r = repo("docs/y");
        round(&skill(
            "code-review",
            &format!("high `{}`。焦点: a", r.dir.display()),
            Path::new("/"),
        ));
        assert!(round(&review(&r)).contains("2 周目"));
    }

    #[test]
    fn branches_are_counted_separately() {
        let r = repo("docs/y");
        round(&review(&r));
        round(&review(&r));
        let other = r.dir.with_file_name("other");
        git(
            &r.dir,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "docs/z",
                other.to_str().unwrap(),
                "main",
            ],
        );
        commit(&other, "z");
        let input = skill(
            "code-review",
            &format!("high {}", other.display()),
            Path::new("/"),
        );
        let context = round(&input);
        assert!(context.contains("1 周目"), "{context}");
        assert!(context.contains("docs/z"), "{context}");
    }

    #[test]
    fn count_survives_a_rebase_of_the_branch() {
        let r = repo("docs/y");
        round(&review(&r));
        git(&r.dir, &["switch", "-q", "main"]);
        commit(&r.dir, "on-main");
        git(&r.dir, &["switch", "-q", "docs/y"]);
        git(&r.dir, &["rebase", "-q", "main"]);
        assert!(round(&review(&r)).contains("2 周目"));
    }

    #[test]
    fn rounds_of_a_merged_branch_do_not_count_for_a_new_branch_of_the_same_name() {
        let r = repo("docs/y");
        round(&review(&r));
        git(&r.dir, &["switch", "-q", "main"]);
        git(&r.dir, &["merge", "-q", "--no-ff", "-m", "merge", "docs/y"]);
        git(&r.dir, &["branch", "-q", "-D", "docs/y"]);
        // 同じ秒の中で作り直すと、ブランチの作成の時刻と前の行の時刻を見分けられない
        std::thread::sleep(std::time::Duration::from_millis(1100));
        git(&r.dir, &["switch", "-q", "-c", "docs/y"]);
        commit(&r.dir, "again");
        assert!(round(&review(&r)).contains("1 周目"));
    }

    #[test]
    fn rounds_before_a_rebase_of_a_merged_branch_do_not_count_for_a_new_branch_of_the_same_name() {
        let r = repo("docs/y");
        round(&review(&r));
        git(&r.dir, &["switch", "-q", "main"]);
        commit(&r.dir, "on-main");
        git(&r.dir, &["switch", "-q", "docs/y"]);
        git(&r.dir, &["rebase", "-q", "main"]);
        git(&r.dir, &["switch", "-q", "main"]);
        git(&r.dir, &["merge", "-q", "--no-ff", "-m", "merge", "docs/y"]);
        git(&r.dir, &["branch", "-q", "-D", "docs/y"]);
        // 同じ秒の中で作り直すと、ブランチの作成の時刻と前の行の時刻を見分けられない
        std::thread::sleep(std::time::Duration::from_millis(1100));
        git(&r.dir, &["switch", "-q", "-c", "docs/y"]);
        commit(&r.dir, "again");
        assert!(round(&review(&r)).contains("1 周目"));
    }

    #[test]
    fn detached_head_is_not_counted() {
        let r = repo("docs/y");
        git(&r.dir, &["switch", "-q", "--detach"]);
        for _ in 0..3 {
            assert!(run(&review(&r)).is_empty());
        }
    }
}
