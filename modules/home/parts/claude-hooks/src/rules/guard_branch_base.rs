//! guard-branch-base: 既定ブランチ以外の上で、起点を省いたブランチ作成を止める
//!
//! 起点を省くと新しいブランチは今の HEAD から作られ、作業中のブランチの未完了のコミット
//! （別のトピック）が入る。既定ブランチの上なら HEAD = 既定ブランチなので止めない。
//! 今のブランチの続きにするのが意図なら、起点を明示すれば（HEAD でも）通る。
//!
//! 対象: `git switch -c|-C <b>`、`git checkout -b|-B <b>`、`git branch <b>`、
//! `git worktree add <path> -b|-B <b>`、`git worktree add <path>`（パスの名前のブランチが
//! 無ければ HEAD から作られる）。いずれも起点（start-point / commit-ish）が無いもの。
//! 既定ブランチが見つからないリポジトリでは起点を示せないので何もしない。

use super::Rule;
use crate::git;
use crate::input::Input;
use crate::output::Finding;
use crate::shell::{Arg, Cmd, Shell};
use std::path::Path;

pub struct GuardBranchBase;

/// 起点を省いたブランチ作成
#[derive(Debug, PartialEq)]
struct NewBranch {
    name: String,
    /// `git worktree add <path>` の暗黙の作成。同名のブランチが既にあればチェックアウトするだけ
    implicit: bool,
    /// 起点に HEAD を明示した同じコマンド（今のブランチの続きにするときの例）
    with_head: String,
}

/// 引数を読んだ結果
#[derive(Default)]
struct Parsed {
    /// ブランチ名を値に取るフラグ（`-c` / `-b` など）の値
    branch: Option<String>,
    /// 位置引数（`--` の後も含む）
    positional: Vec<String>,
    /// それ以外のフラグ
    flags: Vec<String>,
}

/// `create` はブランチ名を値に取るフラグ（長い形は `--x=<b>` も）、`takes_value` は値を飛ばすフラグ
fn read(args: &[Arg], create: &[&str], takes_value: &[&str]) -> Parsed {
    let mut p = Parsed::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let t = a.text.as_str();
        if t == "--" {
            // -- の後はすべて位置引数（`git branch -- <b>` もブランチを作る）
            p.positional.extend(it.map(|v| v.text.clone()));
            break;
        }
        if let Some(v) = create_value(t, create) {
            p.branch = match v {
                Some(inline) => Some(inline),
                None => it.next().map(|v| v.text.clone()),
            };
        } else if takes_value.contains(&t) {
            it.next();
        } else if t.starts_with('-') && t != "-" {
            p.flags.push(t.to_string());
        } else {
            p.positional.push(t.to_string());
        }
    }
    p
}

/// t がブランチ名を値に取るフラグなら Some。値が t に含まれていれば Some(Some(値))、
/// 次の引数なら Some(None)。git の受け付ける書き方に合わせて、くっつけた・まとめた短い形（`-cfeat/a`、`-qc`）と
/// 一意に略した長い形（`--cre feat/a`、`--force-c=feat/a`）も読む。ただし git は完全一致する
/// オプションを優先するので、`--force`（switch の --discard-changes の別名）は --force-create の略ではない
fn create_value(t: &str, create: &[&str]) -> Option<Option<String>> {
    if let Some(long) = t.strip_prefix("--") {
        let (name, value) = match long.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (long, None),
        };
        if name.is_empty() || name == "force" {
            return None;
        }
        let hit = create
            .iter()
            .filter_map(|c| c.strip_prefix("--"))
            .any(|c| c.starts_with(name));
        return hit.then_some(value);
    }
    // 短いオプションのまとまり（`-qc`、`-qcfeat/a`）。作成のオプションの文字までを読み、
    // その後ろが値（空なら次の引数）。対象のコマンドで作成の前に値を取る短いオプションはない
    let cluster = t.strip_prefix('-')?;
    for (i, c) in cluster.char_indices() {
        if create.contains(&format!("-{c}").as_str()) {
            let rest = &cluster[i + c.len_utf8()..];
            return Some((!rest.is_empty()).then(|| rest.to_string()));
        }
    }
    None
}

fn has_any(flags: &[String], names: &[&str]) -> bool {
    flags.iter().any(|f| names.contains(&f.as_str()))
}

/// 起点を省いていれば、HEAD を明示した同じコマンドとともに返す
fn without_start(sub: &str, args: &[Arg], name: String, positional: usize) -> Option<NewBranch> {
    if positional > 0 {
        return None;
    }
    let raw: Vec<&str> = args.iter().map(|a| a.raw.as_str()).collect();
    Some(NewBranch {
        name,
        implicit: false,
        with_head: format!("git {sub} {} HEAD", raw.join(" ")),
    })
}

/// `git <sub> args…` が起点を省いたブランチ作成なら、その内容
fn new_branch(sub: &str, args: &[Arg]) -> Option<NewBranch> {
    match sub {
        "switch" => {
            let p = read(
                args,
                &["-c", "-C", "--create", "--force-create"],
                &["--orphan"],
            );
            if has_any(&p.flags, &["--orphan"]) {
                return None;
            }
            without_start(sub, args, p.branch?, p.positional.len())
        }
        "checkout" => {
            let p = read(args, &["-b", "-B"], &["--orphan"]);
            without_start(sub, args, p.branch?, p.positional.len())
        }
        "branch" => {
            // 作成の形（git branch [--track|--no-track] [-f] <name> [<start>]）に現れるフラグ以外が
            // あれば一覧・削除・リネームなどの別の操作
            let p = read(args, &[], &[]);
            let creating = p.flags.iter().all(|f| {
                matches!(
                    f.as_str(),
                    "-f" | "--force"
                        | "-t"
                        | "--track"
                        | "--no-track"
                        | "--recurse-submodules"
                        | "-q"
                        | "--quiet"
                        | "--create-reflog"
                ) || f.starts_with("--track=")
            });
            if !creating || p.positional.len() != 1 {
                return None;
            }
            without_start(sub, args, p.positional[0].clone(), 0)
        }
        "worktree" => {
            let (first, rest) = args.split_first()?;
            if first.text != "add" {
                return None;
            }
            let p = read(rest, &["-b", "-B"], &["--reason"]);
            if has_any(&p.flags, &["--detach", "-d", "--orphan"]) {
                return None;
            }
            match (p.branch, p.positional.as_slice()) {
                (Some(name), [_path]) => without_start(sub, args, name, 0),
                // パスだけなら、パスの名前のブランチ（無ければ HEAD から作る）
                (None, [path]) => {
                    let name = path.trim_end_matches('/').rsplit('/').next()?.to_string();
                    let raw: Vec<&str> = rest.iter().map(|a| a.raw.as_str()).collect();
                    Some(NewBranch {
                        with_head: format!("git worktree add -b {name} {} HEAD", raw.join(" ")),
                        name,
                        implicit: true,
                    })
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// `git switch` / `git checkout` で移る先のブランチ。ブランチを変えなければ Ok(None)、
/// 移った先が分からなければ（`-`、`--detach`、ローカルにないブランチやファイルの名前）Err(())
fn switched_to(dir: &Path, sub: &str, args: &[Arg]) -> Result<Option<String>, ()> {
    let create: &[&str] = match sub {
        "switch" => &["-c", "-C", "--create", "--force-create", "--orphan"],
        "checkout" => &["-b", "-B", "--orphan"],
        _ => return Ok(None),
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let t = a.text.as_str();
        if let Some(v) = create_value(t, create) {
            // 新しいブランチを作ってそこへ移る
            return match v {
                Some(inline) => Ok(Some(inline)),
                None => it.next().map(|v| Some(v.text.clone())).ok_or(()),
            };
        }
    }
    let p = read(args, &[], &[]);
    if sub == "checkout" && args.iter().any(|a| a.text == "--") {
        // `git checkout [<tree-ish>] -- <paths>` はファイルを戻すだけ
        return Ok(None);
    }
    if has_any(&p.flags, &["--detach", "-d"]) {
        return Err(());
    }
    match p.positional.first() {
        Some(b) if b != "-" => {
            let r = format!("refs/heads/{b}");
            match git::git(dir, &["show-ref", "--verify", "--quiet", &r]) {
                Some(_) => Ok(Some(b.clone())),
                None => Err(()),
            }
        }
        Some(_) => Err(()),
        None => Ok(None),
    }
}

/// cmd を実行する時点の今のブランチ。同じシェルのスコープで cmd より前にあり、同じディレクトリを
/// 対象にする git switch / checkout の後のブランチ（`git switch main && git merge …` の main）。
/// 移った先が分からなければ None。detached HEAD は空文字列
pub(super) fn branch_at(shell: &Shell, cmd: &Cmd, base: &Path, dir: &Path) -> Option<String> {
    let mut branch = Some(git::current_branch(dir));
    for c in shell.commands() {
        if c.start >= cmd.start {
            break;
        }
        if c.scope != cmd.scope || shell.target_dir(&c, base) != dir {
            continue;
        }
        if let Some((sub, args)) = c.git_subcommand() {
            match switched_to(dir, sub, args) {
                Ok(Some(b)) => branch = Some(b),
                Ok(None) => {}
                Err(()) => branch = None,
            }
        }
    }
    branch
}

impl Rule for GuardBranchBase {
    fn name(&self) -> &'static str {
        "guard-branch-base"
    }

    fn check(&self, input: &Input, shell: &Shell) -> Vec<Finding> {
        for cmd in shell.commands() {
            let Some(nb) = cmd
                .git_subcommand()
                .and_then(|(sub, args)| new_branch(sub, args))
            else {
                continue;
            };
            let dir = shell.target_dir(&cmd, &input.cwd);
            if git::git(&dir, &["rev-parse", "--git-dir"]).is_none() {
                continue;
            }
            let ref_name = format!("refs/heads/{}", nb.name);
            if nb.implicit
                && git::git(&dir, &["show-ref", "--verify", "--quiet", &ref_name]).is_some()
            {
                continue;
            }
            let Some(default_ref) = git::default_branch(&dir) else {
                continue;
            };
            let default = git::local_name(&default_ref);
            let here = match branch_at(shell, &cmd, &input.cwd, &dir) {
                Some(current) if current == default => continue,
                Some(current) if current.is_empty() => "detached HEAD".to_string(),
                Some(current) => current,
                // 前の git switch で移った先が分からない。既定ブランチとは確かめられないので止める
                None => "（同じコマンドの前の git switch / checkout の移り先）".to_string(),
            };
            let b = &nb.name;
            return vec![Finding::Deny(format!(
                "今のブランチ {here} は既定ブランチ（{default}）ではありません。起点を省くと新しいブランチ {b} は\
                 今の HEAD から作られ、{here} の未完了のコミット（別のトピック）が入ります。\n\
                 - 別のトピックなら既定ブランチから作る: ~/.claude/scripts/worktree-add.sh {b}（worktree を作る）か git branch {b} {default}\n\
                 - 今のブランチの続きにするのが意図なら、起点を明示する（HEAD でもよい）: {}",
                nb.with_head
            ))];
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> Option<NewBranch> {
        let cs = Shell::parse(src).commands();
        let (sub, args) = cs[0].git_subcommand()?;
        new_branch(sub, args)
    }

    fn name(src: &str) -> Option<String> {
        parse(src).map(|b| b.name)
    }

    #[test]
    fn switch_create_without_start() {
        let b = parse("git switch -c feat/a").unwrap();
        assert_eq!(b.name, "feat/a");
        assert!(!b.implicit);
        assert_eq!(b.with_head, "git switch -c feat/a HEAD");
        assert_eq!(name("git switch -q -C feat/a").as_deref(), Some("feat/a"));
        assert_eq!(
            name("git switch --create=feat/a").as_deref(),
            Some("feat/a")
        );
        assert_eq!(
            name("git switch --force-create feat/a").as_deref(),
            Some("feat/a")
        );
    }

    #[test]
    fn attached_and_abbreviated_create_options() {
        assert_eq!(name("git switch -cfeat/a").as_deref(), Some("feat/a"));
        assert_eq!(name("git switch --cre feat/a").as_deref(), Some("feat/a"));
        assert_eq!(
            name("git switch --force-c=feat/a").as_deref(),
            Some("feat/a")
        );
        assert_eq!(name("git checkout -Bfeat/a").as_deref(), Some("feat/a"));
        assert_eq!(
            name("git worktree add -bfeat/a ../wt").as_deref(),
            Some("feat/a")
        );
        assert_eq!(name("git switch -qc feat/a").as_deref(), Some("feat/a"));
        assert_eq!(name("git switch -qcfeat/a").as_deref(), Some("feat/a"));
        assert_eq!(name("git checkout -qb feat/a").as_deref(), Some("feat/a"));
        assert_eq!(
            name("git worktree add -fqb feat/a ../wt").as_deref(),
            Some("feat/a")
        );
        assert_eq!(parse("git switch -qc feat/a main"), None);
        assert_eq!(parse("git switch -cfeat/a main"), None);
        assert_eq!(parse("git switch --conflict=merge main"), None);
        assert_eq!(parse("git switch -日"), None);
    }

    #[test]
    fn switch_with_start_or_without_create_is_not_a_target() {
        assert_eq!(parse("git switch -c feat/a main"), None);
        assert_eq!(parse("git switch -c feat/a --no-track origin/main"), None);
        assert_eq!(parse("git switch main"), None);
        assert_eq!(parse("git switch -"), None);
        assert_eq!(parse("git switch --orphan fresh"), None);
    }

    #[test]
    fn checkout_new_branch() {
        let b = parse("git checkout -b feat/a").unwrap();
        assert_eq!(b.with_head, "git checkout -b feat/a HEAD");
        assert_eq!(name("git checkout -B feat/a").as_deref(), Some("feat/a"));
        assert_eq!(parse("git checkout -b feat/a origin/main"), None);
        assert_eq!(parse("git checkout main"), None);
        assert_eq!(parse("git checkout -- a.txt"), None);
        assert_eq!(parse("git checkout --orphan fresh"), None);
    }

    #[test]
    fn branch_create() {
        let b = parse("git branch feat/a").unwrap();
        assert_eq!(b.with_head, "git branch feat/a HEAD");
        assert_eq!(
            name("git branch --no-track feat/a").as_deref(),
            Some("feat/a")
        );
        assert_eq!(name("git branch -f feat/a").as_deref(), Some("feat/a"));
        assert_eq!(name("git branch -- feat/a").as_deref(), Some("feat/a"));
        assert_eq!(name("git switch -c feat/a --").as_deref(), Some("feat/a"));
        assert_eq!(parse("git switch -c feat/a -- main"), None);
        assert_eq!(name("git branch -qf feat/a").as_deref(), Some("feat/a"));
        assert_eq!(name("git branch -ft feat/a").as_deref(), Some("feat/a"));
        assert_eq!(parse("git branch -qd feat/a"), None);
        assert_eq!(parse("git branch feat/a main"), None);
        assert_eq!(parse("git branch -t feat/a origin/feat/a"), None);
    }

    #[test]
    fn branch_other_modes_are_not_a_target() {
        for src in [
            "git branch",
            "git branch -a",
            "git branch -vv",
            "git branch -d feat/a",
            "git branch -D feat/a",
            "git branch --delete feat/a",
            "git branch -m feat/b",
            "git branch -M feat/a feat/b",
            "git branch -c feat/b",
            "git branch --show-current",
            "git branch --list 'feat/*'",
            "git branch --contains HEAD",
            "git branch --merged main",
            "git branch -u origin/main",
            "git branch --set-upstream-to=origin/main",
            "git branch --unset-upstream",
            "git branch --edit-description",
        ] {
            assert_eq!(parse(src), None, "{src}");
        }
    }

    #[test]
    fn worktree_add() {
        let b = parse("git worktree add ../wt -b feat/a").unwrap();
        assert_eq!(b.name, "feat/a");
        assert!(!b.implicit);
        assert_eq!(b.with_head, "git worktree add ../wt -b feat/a HEAD");
        assert_eq!(
            name("git worktree add -B feat/a ../wt").as_deref(),
            Some("feat/a")
        );
        assert_eq!(
            name("git worktree add --lock --reason 'x y' -b feat/a ../wt").as_deref(),
            Some("feat/a")
        );
        assert_eq!(parse("git worktree add ../wt -b feat/a main"), None);
        assert_eq!(parse("git worktree add -b feat/a ../wt HEAD"), None);
        assert_eq!(parse("git worktree add ../wt feat/a"), None);
        assert_eq!(parse("git worktree add --detach ../wt"), None);
        assert_eq!(parse("git worktree add --orphan -b fresh ../wt"), None);
        assert_eq!(parse("git worktree list"), None);
        assert_eq!(parse("git worktree remove ../wt"), None);
    }

    #[test]
    fn worktree_add_path_only_creates_branch_named_after_path() {
        let b = parse("git worktree add ../x/newname").unwrap();
        assert_eq!(b.name, "newname");
        assert!(b.implicit);
        assert_eq!(b.with_head, "git worktree add -b newname ../x/newname HEAD");
    }
}
