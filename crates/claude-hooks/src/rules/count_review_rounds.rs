//! count-review-rounds: ローカルの /code-review の周をブランチごとに数え、何周目かをモデルに伝える（止めない）
//!
//! 1 周目の後にもう 1 周回すかは、修正の大きさと危険からモデルが決める（/git-merge の「直す指摘の基準」）。数えるのは
//! 決定的なので hook が行い、判断の材料として周の数を渡す。周の数で止めないのは、確かめた誤りの修正まで数で止めると、
//! 修正が誰にも読まれずに残るか、別の名前のブランチに移す回り道を誘うため。
//!
//! - 数える呼び出し: Skill の `code-review` だけ。対象は引数の中で最初にある、ディレクトリとして在る絶対パス（無ければ
//!   hook の cwd）で、その worktree がチェックアウトしているブランチを数える。親が Agent で担当にレビューを頼む周も、
//!   担当が呼ぶ Skill で数える。Agent の prompt は見ない（自然文から「/code-review を頼む」を見分ける判定は、
//!   否定や別のスキルを呼ぶ文に当たるか、頼む文を落とした）
//! - 置き場: リポジトリの共通の git ディレクトリの `claude-review-rounds/<ブランチ名>`（`/` は `%2F`）。1 行が 1 周で、
//!   回したときの HEAD と時刻と、呼んだセッションの ID と、回した worktree を書く。worktree を消して作り直しても、rebase で HEAD が変わっても数は残る。ブランチの
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
        .map(|w| PathBuf::from(crate::shell::native(w).as_ref()))
        .filter(|p| p.is_absolute())
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

/// dir の clone の、周の数の置き場（共通の git ディレクトリの下。worktree をまたいで 1 つ）。dir が git のリポジトリの
/// 中でなければ None
fn state_dir(dir: &Path) -> Option<PathBuf> {
    // リポジトリを指す GIT_* を継承しない（git の hook の中や、そこから起動したセッションでは GIT_DIR が渡り、dir の
    // clone でなくそのリポジトリを引く）。探す範囲を止める GIT_CEILING_DIRECTORIES などは残す
    let out = std::process::Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let common = String::from_utf8_lossy(&out.stdout);
    let common = common.trim_end_matches('\n');
    (out.status.success() && !common.is_empty()).then(|| Path::new(common).join(STATE_DIR))
}

/// 置き場の 1 行（1 周）。`at` は呼んだときの UNIX 秒、`session` は呼んだ Claude Code のセッションの ID、`dir` は回した
/// worktree（どちらも、書く前の版の行と、ID の無い入力では None）
pub(crate) struct Round {
    pub(crate) head: String,
    pub(crate) at: Option<u64>,
    pub(crate) session: Option<String>,
    pub(crate) dir: Option<PathBuf>,
}

/// dir の clone の、周の数の置き場と、ファイル（ブランチ）ごとの行（書いた順）。dir が git のリポジトリの中でなければ None
pub(crate) fn rounds(dir: &Path) -> Option<(PathBuf, Vec<Vec<Round>>)> {
    let state_dir = state_dir(dir)?;
    let files = std::fs::read_dir(&state_dir)
        .map(|files| {
            files
                .flatten()
                .filter_map(|file| std::fs::read_to_string(file.path()).ok())
                .map(|text| {
                    text.lines()
                        .filter_map(|l| {
                            // worktree は空白を含みうるので、最後の欄として行の残りを取る
                            let mut fields = l.splitn(4, ' ');
                            Some(Round {
                                head: fields.next()?.to_string(),
                                at: fields.next().and_then(|t| t.parse().ok()),
                                session: fields
                                    .next()
                                    .filter(|s| !s.is_empty())
                                    .map(str::to_string),
                                dir: fields.next().filter(|d| !d.is_empty()).map(PathBuf::from),
                            })
                        })
                        .collect()
                })
                .collect()
        })
        .unwrap_or_default();
    Some((state_dir, files))
}

/// dir の clone の、周の数の置き場と、そこに head を HEAD にして /code-review を呼んだ行があるか。ブランチは問わない
/// （同じ SHA なら同じコミット。ブランチの名前を変えても引ける）。dir が git のリポジトリの中でなければ None。
/// 行があるのは呼んだことで、レビューが対象を読めたことまでは示さない（[`CountReviewRounds`] は結果を見ない）
pub(crate) fn round_ran_at(dir: &Path, head: &str) -> Option<(PathBuf, bool)> {
    let (state_dir, files) = rounds(dir)?;
    let found = files.iter().flatten().any(|r| r.head == head);
    Some((state_dir, found))
}

fn context(branch: &str, round: usize) -> String {
    format!(
        "ブランチ {branch} のローカルの /code-review の {round} 周目です（担当と親の周の合計。rebase の後の周も数えます）。\
         周の数では止めません。直した後にもう 1 周回すかは、修正の大きさと危険で決めます（/git-merge の「直す指摘の基準」の「終わり」の項）。"
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
        let Some(state_dir) = state_dir(&dir) else {
            return Vec::new();
        };
        let path = state_dir.join(state_file_name(&branch));
        let mut lines = live_rounds(&dir, &branch, &path);
        let head = git::git(&dir, &["rev-parse", "HEAD"]).unwrap_or_default();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // セッションは、record-local-review が 0 件の記録の前に、この周の /code-review の記録（サブエージェントの
        // transcript）を探すのに使う。空白を含む値は行の区切りを崩すので書かない
        let session = &input.session_id;
        if session.is_empty() || session.contains(char::is_whitespace) {
            lines.push(format!("{head} {now}"));
        } else {
            lines.push(format!("{head} {now} {session} {}", dir.display()));
        }
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
    use super::super::test_util::TempDir;
    use super::*;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn git(dir: &Path, args: &[&str]) {
        // CI の runner のように、グローバルの設定に名前とメールが無い所でもコミットできるようにする
        let ok = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
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
                Finding::Prepend { .. } => panic!("周の数ではコマンドを書き換えないはず"),
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

    #[cfg(windows)]
    #[test]
    fn git_bash_drive_path_names_the_review_target() {
        let r = repo("feat/msys");
        let args = format!("high {}", crate::shell::to_msys(&r.dir));
        let context = round(&skill("code-review", &args, Path::new(r"C:\")));
        assert!(context.contains("feat/msys"), "{context}");
    }

    #[test]
    fn the_line_names_the_session_that_ran_the_round() {
        // record-local-review は、0 件の記録の前に、このセッションの /code-review の記録（サブエージェントの transcript）を
        // 探して差分を読んだかを確かめる
        let r = repo("feat/s");
        let mut input = review(&r);
        input.session_id = "0a87f718-session".into();
        round(&input);
        let head = git_out(&r.dir, &["rev-parse", "HEAD"]);
        let (_, files) = rounds(&r.dir).expect("clone の中");
        let lines: Vec<&Round> = files.iter().flatten().collect();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].head, head);
        assert_eq!(lines[0].session.as_deref(), Some("0a87f718-session"));
        // 回した worktree（空白を含んでもよい）。record-local-review が、同じ頃に回したほかの worktree の周と分ける
        assert_eq!(lines[0].dir.as_deref(), Some(r.dir.as_path()));
        // 前の版の hook が書いた、セッションの無い行も読む
        let state = state_dir(&r.dir).unwrap();
        std::fs::write(state.join("old"), format!("{head} 1794900000\n")).unwrap();
        let (_, files) = rounds(&r.dir).unwrap();
        let old: Vec<&Round> = files
            .iter()
            .flatten()
            .filter(|l| l.session.is_none())
            .collect();
        assert_eq!(old.len(), 1);
        assert_eq!(old[0].at, Some(1794900000));
    }

    fn git_out(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_string()
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
