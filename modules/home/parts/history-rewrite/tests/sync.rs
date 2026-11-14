//! 書き換えた ref を生きているリポジトリに移した後、`history-rewrite sync-worktrees` が各 worktree を新しい履歴に
//! 合わせること（ADR-0045 の「## 変更」の 2026-11-11、ADR-0088）。test_history_rewrite.py の SyncWorktrees と同じ
//! worktree を作る。
//!
//! 第 2 段と worktree の番号の付け直しの `commit-refs` は PATH のものを使う。辞書の語（崩れた語）をわざと含むので、
//! .textlintignore で ja-terms の検査から外す。

use history_rewrite::run::read_map;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;

/// 履歴に無いコミットの番号。対応表で「消えた」（新が 0）にすると、それを指す文書で commit-refs remap が止まる
/// HEAD、index、印、MERGE_HEAD、作業ツリーのファイル
type State = (
    String,
    String,
    String,
    Option<Vec<u8>>,
    BTreeMap<String, Vec<u8>>,
);

const VANISHED: &str = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef";

fn env<'a>(cmd: &'a mut Command, root: &Path) -> &'a mut Command {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(key);
        }
    }
    cmd.env("HOME", root.join("home"))
        .env("USERPROFILE", root.join("home"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
}

#[derive(Clone)]
struct Repo {
    root: PathBuf,
    path: PathBuf,
}

impl Repo {
    fn output(&self, args: &[&str], input: Option<&str>) -> Output {
        use std::io::Write;
        let mut child = env(&mut Command::new("git"), &self.root)
            .arg("-C")
            .arg(&self.path)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.unwrap_or_default().as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    fn git_in(&self, args: &[&str], input: &str) -> String {
        let out = self.output(args, Some(input));
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    fn git(&self, args: &[&str]) -> String {
        self.git_in(args, "")
    }

    fn write(&self, name: &str, content: &str) {
        std::fs::write(self.path.join(name), content).unwrap();
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.path.join(name)).unwrap()
    }

    fn commit(&self, message: &str) -> String {
        self.git(&["add", "-A"]);
        self.git_in(&["commit", "-q", "--no-verify", "-F", "-"], message);
        self.head()
    }

    fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"]).trim().to_string()
    }

    fn tracked_changes(&self) -> String {
        self.git(&["status", "--porcelain", "--untracked-files=no"])
    }

    /// HEAD、index（印を含む）、MERGE_HEAD、作業ツリーのファイルの中身
    fn state(&self) -> State {
        let merge_head = self
            .path
            .join(self.git(&["rev-parse", "--git-path", "MERGE_HEAD"]).trim());
        let mut files = BTreeMap::new();
        for entry in std::fs::read_dir(&self.path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() && entry.file_name() != ".git" {
                files.insert(
                    entry.file_name().to_string_lossy().into_owned(),
                    std::fs::read(entry.path()).unwrap(),
                );
            }
        }
        (
            self.head(),
            self.git(&["ls-files", "-s"]),
            self.git(&["ls-files", "-v"]),
            std::fs::read(&merge_head).ok(),
            files,
        )
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

struct Fixture {
    root: PathBuf,
    workdir: PathBuf,
    first: String,
    second: String,
    map: HashMap<String, String>,
    repo: Repo,
    w: HashMap<&'static str, Repo>,
    partial_before: (String, Vec<u8>),
    before: HashMap<&'static str, State>,
    status: Option<i32>,
    stdout: String,
    stderr: String,
}

fn snapshot(w: &Repo) -> (String, Vec<u8>) {
    (
        w.git(&["ls-files", "-s"]),
        std::fs::read(w.path.join("a.md")).unwrap(),
    )
}

fn history_rewrite(root: &Path, args: &[&str], extra_env: &[(&str, &str)]) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_history-rewrite"));
    env(&mut cmd, root).args(args);
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("home")).unwrap();
        std::fs::write(
            root.join("home").join(".gitconfig"),
            "[init]\n\tdefaultBranch = main\n[user]\n\tname = t\n\temail = t@t\n[commit]\n\tgpgsign = false\n",
        )
        .unwrap();
        let repo = Repo {
            root: root.clone(),
            path: root.join("source"),
        };
        std::fs::create_dir_all(&repo.path).unwrap();
        repo.git(&["init", "-q"]);
        repo.write("a.md", "確かめの記録\n");
        repo.write("b.md", "そのまま\n");
        let first = repo.commit("docs: a\n");
        repo.write("notes.md", &format!("{} の記録\n", &first[..8]));
        let second = repo.commit("docs: notes\n");

        let worktree = |name: &str, option: &str, commit: &str| -> Repo {
            let path = root.join(name);
            repo.git(&["worktree", "add", "-q", option, &path.display().to_string(), commit]);
            Repo {
                root: root.clone(),
                path,
            }
        };
        let mut w: HashMap<&'static str, Repo> = HashMap::new();
        // 変更の無い worktree（メインと、detached HEAD）。追跡していないファイルは残る
        repo.write("scratch.log", "確かめの控え\n");
        w.insert("detached", worktree("detached", "--detach", &first));
        // 変更のある worktree: 書き換えたファイルの変更、書き換えないファイルの変更、ステージした新しいファイル、
        // 消したファイル
        let changed = worktree("changed", "-bchanged", "HEAD");
        changed.write("a.md", "確かめの記録\n確かめの追記\n");
        changed.write("b.md", &format!("そのまま\n{} を見る\n", &first[..8]));
        changed.write("new.md", &format!("{} の確かめの記録\n", &second[..8]));
        changed.git(&["add", "new.md"]);
        std::fs::remove_file(changed.path.join("notes.md")).unwrap();
        w.insert("changed", changed);
        // 部分のステージがある worktree（機械では合わせられない）
        let partial = worktree("partial", "-bpartial", "HEAD");
        partial.write("a.md", "確かめの記録\nステージした\n");
        partial.git(&["add", "a.md"]);
        partial.write("a.md", "確かめの記録\nステージした\nしていない\n");
        w.insert("partial", partial);

        // 未コミットの変更を git が普通の比べ方では見せない worktree（どれも ref を移す前の HEAD の detached）
        let assumed = worktree("assumed", "--detach", &second);
        assumed.git(&["update-index", "--assume-unchanged", "a.md"]);
        assumed.write("a.md", "確かめの記録\n印の下の編集\n");
        w.insert("assumed", assumed);
        // git rm --cached で、ファイルを残したまま追跡から外した
        let removed = worktree("removed", "--detach", &second);
        removed.git(&["rm", "-q", "--cached", "a.md"]);
        w.insert("removed", removed);
        // 衝突を解いてステージしたマージの途中（MERGE_HEAD が古い履歴のコミットを指す）
        let side = worktree("side-branch", "-bside", &first);
        side.write("notes.md", "横の記録\n");
        side.commit("docs: side notes\n");
        repo.git(&["worktree", "remove", &side.path.display().to_string()]);
        let merging = worktree("merging", "--detach", &second);
        assert!(!merging.output(&["merge", "-q", "side"], None).status.success(), "notes.md の add/add で衝突するはず");
        merging.write("notes.md", &format!("{} の記録\n横の記録\n", &first[..8]));
        merging.git(&["add", "notes.md"]);
        w.insert("merging", merging);
        // git add -N で足すことだけを記した新しいファイル（中身はステージしていない）
        let intent = worktree("intent", "--detach", &second);
        intent.write("intent.md", &format!("{} を見る\n", &second[..8]));
        intent.git(&["add", "-N", "intent.md"]);
        w.insert("intent", intent);
        // ほかのプロセスが index.lock を持つ
        w.insert("locked", worktree("locked", "--detach", &second));
        // remap が止まる番号（対応表で消えたコミット）をステージした。対応表は test の中で足す
        let remapfail = worktree("remapfail", "--detach", &second);
        remapfail.write("a.md", "確かめの記録\n確かめの追記\n");
        remapfail.write("vanished.md", &format!("{} を見る\n", &VANISHED[..12]));
        remapfail.git(&["add", "a.md", "vanished.md"]);
        w.insert("remapfail", remapfail);
        // 書き込みの途中で git が失敗する（reference-transaction の hook が HEAD の移動を拒む）
        let broken = worktree("broken", "--detach", &second);
        broken.write("a.md", "確かめの記録\n確かめの追記\n");
        w.insert("broken", broken);
        // 書き換えたファイルを編集して、読み出し専用にした
        let readonly = worktree("readonly", "--detach", &second);
        readonly.write("a.md", "確かめの記録\n確かめの追記\n");
        let mut permissions = std::fs::metadata(readonly.path.join("a.md")).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(readonly.path.join("a.md"), permissions).unwrap();
        w.insert("readonly", readonly);
        // 操作の印の無い衝突の項（stash pop や apply --3way の衝突と同じ index）
        let unmerged = worktree("unmerged", "--detach", &second);
        let blob = unmerged.git(&["rev-parse", "HEAD:b.md"]);
        let mut info = format!("0 {}\tb.md\n", "0".repeat(40));
        for stage in 1..=3 {
            info.push_str(&format!("100644 {} {stage}\tb.md\n", blob.trim()));
        }
        unmerged.git_in(&["update-index", "--index-info"], &info);
        w.insert("unmerged", unmerged);
        // git add -N で足した後に、ファイルを消した
        let intent_gone = worktree("intent-gone", "--detach", &second);
        intent_gone.write("gone.md", "消す\n");
        intent_gone.git(&["add", "-N", "gone.md"]);
        std::fs::remove_file(intent_gone.path.join("gone.md")).unwrap();
        w.insert("intent-gone", intent_gone);

        let workdir = root.join("work");
        let words = ["--exclude", "**/never.md"];
        let (source, work) = (repo.path.display().to_string(), workdir.display().to_string());
        let run = history_rewrite(&root, &[&["run", "--source", &source, "--workdir", &work][..], &words].concat(), &[]);
        assert!(run.status.success(), "{}", text(&run.stderr));
        let map = read_map(&workdir.join("commit-map")).unwrap();

        // スキルの手順 4: ref を移す前に、各 worktree の HEAD と控えのパッチを残し、ref を 1 つのトランザクションで移す
        let mut rows = String::new();
        let mut synced: Vec<Repo> = vec![repo.clone()];
        for name in ["detached", "changed", "partial", "assumed", "removed", "merging", "intent", "locked", "unmerged", "intent-gone"] {
            synced.push(w[name].clone());
        }
        for (k, wt) in synced.iter().enumerate() {
            let patch = workdir.join(format!("worktree-{}.patch", k + 1));
            std::fs::write(&patch, wt.git(&["diff", "HEAD"])).unwrap();
            rows.push_str(&format!("{}\t{}\t{}\n", wt.path.display(), wt.head(), patch.display()));
        }
        std::fs::write(workdir.join("worktree-state"), rows).unwrap();
        let rewritten = Repo {
            root: root.clone(),
            path: workdir.join("rewritten"),
        };
        let mut transaction = String::new();
        for b in ["main", "changed", "partial"] {
            transaction.push_str(&format!(
                "update refs/heads/{b} {} {}\n",
                rewritten.git(&["rev-parse", b]).trim(),
                repo.git(&["rev-parse", b]).trim()
            ));
        }
        repo.git(&["fetch", "-q", &rewritten.path.display().to_string(), "+refs/heads/*:refs/rewritten/*"]);
        repo.git_in(&["update-ref", "--stdin"], &transaction);

        let partial_before = snapshot(&w["partial"]);
        let lock = w["locked"].git(&["rev-parse", "--git-path", "index.lock"]);
        std::fs::write(w["locked"].path.join(lock.trim()), b"").unwrap();
        let before = ["assumed", "merging", "locked", "remapfail"]
            .into_iter()
            .map(|name| (name, w[name].state()))
            .collect();
        let result = history_rewrite(&root, &[&["sync-worktrees", "--workdir", &work][..], &words].concat(), &[]);
        Fixture {
            root: root.clone(),
            workdir,
            first,
            second,
            map,
            repo,
            w,
            partial_before,
            before,
            status: result.status.code(),
            stdout: text(&result.stdout),
            stderr: text(&result.stderr),
        }
    })
}

impl Fixture {
    fn path(&self, name: &str) -> String {
        self.w[name].path.display().to_string()
    }

    /// w だけを行に持つ worktree-state と、map_lines を足した対応表で sync-worktrees を実行する
    fn run_sync(&self, name: &str, map_lines: &str, extra_env: &[(&str, &str)]) -> Output {
        let w = &self.w[name];
        let workdir = self.root.join(format!("work-{name}"));
        std::fs::create_dir_all(&workdir).unwrap();
        let map = std::fs::read_to_string(self.workdir.join("commit-map")).unwrap();
        std::fs::write(workdir.join("commit-map"), map + map_lines).unwrap();
        std::fs::write(
            workdir.join("worktree-state"),
            format!(
                "{}\t{}\t{}\n",
                w.path.display(),
                w.head(),
                workdir.join("worktree-1.patch").display()
            ),
        )
        .unwrap();
        history_rewrite(
            &self.root,
            &[
                "sync-worktrees",
                "--workdir",
                &workdir.display().to_string(),
            ],
            extra_env,
        )
    }
}

#[test]
fn a_worktree_without_changes_matches_the_new_history() {
    let f = fixture();
    assert!(
        f.stdout
            .contains(&format!("RESET: {}\n", f.repo.path.display())),
        "{}",
        f.stdout
    );
    assert_eq!(f.repo.tracked_changes(), "");
    assert_eq!(f.repo.read("a.md"), "確認の記録\n");
}

#[test]
fn untracked_files_are_kept() {
    assert_eq!(fixture().repo.read("scratch.log"), "確かめの控え\n");
}

#[test]
fn a_detached_head_moves_to_the_mapped_commit() {
    let f = fixture();
    assert!(
        f.stdout
            .contains(&format!("RESET: {}\n", f.path("detached"))),
        "{}",
        f.stdout
    );
    assert_eq!(f.w["detached"].head(), f.map[&f.first]);
    assert_eq!(f.w["detached"].tracked_changes(), "");
}

#[test]
fn uncommitted_changes_are_kept_with_the_same_transform() {
    let f = fixture();
    assert!(
        f.stdout.contains(&format!("KEPT: {}", f.path("changed"))),
        "{}",
        f.stdout
    );
    let w = &f.w["changed"];
    // 書き換えたファイルの変更には語の変換を当て、書き換えないファイルの変更は番号だけを付け直す
    assert_eq!(w.read("a.md"), "確認の記録\n確認の追記\n");
    assert_eq!(
        w.read("b.md"),
        format!("そのまま\n{} を見る\n", &f.map[&f.first][..8])
    );
    assert!(!w.path.join("notes.md").exists());
    assert_eq!(
        w.read("new.md"),
        format!("{} の確かめの記録\n", &f.map[&f.second][..8])
    );
}

#[test]
fn staged_and_unstaged_changes_keep_their_state() {
    let f = fixture();
    let w = &f.w["changed"];
    let status = w.tracked_changes();
    let mut lines: Vec<&str> = status.lines().collect();
    lines.sort();
    assert_eq!(lines, [" D notes.md", " M a.md", " M b.md", "A  new.md"]);
    assert!(w.git(&["show", ":new.md"]).contains(&f.map[&f.second][..8]));
}

#[test]
fn a_worktree_with_partly_staged_changes_is_left_as_it_was() {
    let f = fixture();
    assert!(
        f.stdout.contains(&format!("LEFT: {}", f.path("partial"))),
        "{}",
        f.stdout
    );
    assert_eq!(snapshot(&f.w["partial"]), f.partial_before);
    assert_eq!(f.status, Some(1), "{}", f.stderr);
}

#[test]
fn a_worktree_whose_commit_is_not_in_the_map_is_left() {
    let f = fixture();
    let workdir = f.root.join("unmapped");
    std::fs::create_dir_all(&workdir).unwrap();
    std::fs::copy(f.workdir.join("commit-map"), workdir.join("commit-map")).unwrap();
    let orphan = "0".repeat(40);
    std::fs::write(
        workdir.join("worktree-state"),
        format!(
            "{}\t{orphan}\t{}\n",
            f.path("detached"),
            workdir.join("worktree-1.patch").display()
        ),
    )
    .unwrap();
    let out = history_rewrite(
        &f.root,
        &[
            "sync-worktrees",
            "--workdir",
            &workdir.display().to_string(),
        ],
        &[],
    );
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(
        stdout.contains(&format!("LEFT: {}", f.path("detached"))) && stdout.contains(&orphan),
        "{stdout}"
    );
}

#[test]
fn an_edit_under_assume_unchanged_is_left_as_it_was() {
    let f = fixture();
    assert!(
        f.stdout.contains(&format!("LEFT: {}", f.path("assumed"))),
        "{}",
        f.stdout
    );
    assert_eq!(f.w["assumed"].state(), f.before["assumed"]);
}

#[test]
fn a_path_removed_with_rm_cached_stays_removed_from_the_index() {
    let f = fixture();
    assert!(
        f.stdout.contains(&format!("KEPT: {}", f.path("removed"))),
        "{}",
        f.stdout
    );
    let w = &f.w["removed"];
    assert_eq!(w.git(&["ls-files", "a.md"]), "");
    let status = w.git(&["status", "--porcelain", "--", "a.md"]);
    assert_eq!(status.lines().collect::<Vec<_>>(), ["D  a.md", "?? a.md"]);
    // 追跡していないファイルは、変えずに残す
    assert_eq!(w.read("a.md"), "確かめの記録\n");
}

#[test]
fn a_merge_with_resolved_conflicts_is_left_as_it_was() {
    let f = fixture();
    assert!(
        f.stdout.contains(&format!("LEFT: {}", f.path("merging"))),
        "{}",
        f.stdout
    );
    assert_eq!(f.w["merging"].state(), f.before["merging"]);
}

#[test]
fn an_intent_to_add_file_stays_intent_to_add() {
    let f = fixture();
    assert!(
        f.stdout.contains(&format!("KEPT: {}", f.path("intent"))),
        "{}",
        f.stdout
    );
    let w = &f.w["intent"];
    assert_eq!(w.head(), f.map[&f.second]);
    assert_eq!(w.tracked_changes(), " A intent.md\n");
    assert_eq!(
        w.read("intent.md"),
        format!("{} を見る\n", &f.map[&f.second][..8])
    );
}

#[test]
fn a_worktree_whose_index_is_locked_is_left_as_it_was() {
    let f = fixture();
    assert!(
        f.stdout.contains(&format!("LEFT: {}", f.path("locked"))),
        "{}",
        f.stdout
    );
    assert_eq!(f.w["locked"].state(), f.before["locked"]);
}

#[test]
fn a_worktree_where_remap_stops_is_left_as_it_was() {
    let f = fixture();
    let out = f.run_sync(
        "remapfail",
        &format!("{VANISHED} {}\n", "0".repeat(40)),
        &[],
    );
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    assert!(
        text(&out.stdout).contains(&format!("LEFT: {}", f.path("remapfail"))),
        "{}",
        text(&out.stdout)
    );
    assert_eq!(f.w["remapfail"].state(), f.before["remapfail"]);
}

#[test]
fn a_read_only_changed_file_is_converted() {
    let f = fixture();
    let out = f.run_sync("readonly", "", &[]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    assert!(
        text(&out.stdout).contains(&format!("KEPT: {}", f.path("readonly"))),
        "{}",
        text(&out.stdout)
    );
    let a = f.w["readonly"].path.join("a.md");
    assert_eq!(
        std::fs::read_to_string(&a).unwrap(),
        "確認の記録\n確認の追記\n"
    );
    assert!(std::fs::metadata(&a).unwrap().permissions().readonly());
}

#[test]
fn conflict_entries_without_an_operation_are_left_as_a_conflict() {
    let f = fixture();
    assert!(
        f.stdout
            .contains(&format!("LEFT: {} (衝突の途中", f.path("unmerged"))),
        "{}",
        f.stdout
    );
}

#[test]
fn a_deleted_intent_to_add_file_is_not_a_partial_stage() {
    let f = fixture();
    assert!(
        f.stdout
            .contains(&format!("KEPT: {}", f.path("intent-gone"))),
        "{}",
        f.stdout
    );
    assert_eq!(
        f.w["intent-gone"].git(&["ls-files", "gone.md"]),
        "gone.md\n"
    );
}

#[cfg(unix)]
#[test]
fn a_git_failure_while_writing_names_the_worktree_and_keeps_the_changes() {
    use std::os::unix::fs::PermissionsExt;
    let f = fixture();
    let hooks = f.root.join("hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    let hook = hooks.join("reference-transaction");
    std::fs::write(&hook, "#!/bin/sh\n[ \"$1\" != prepared ]\n").unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let hooks_s = hooks.display().to_string();
    let config = [
        ("GIT_CONFIG_COUNT", "1"),
        ("GIT_CONFIG_KEY_0", "core.hooksPath"),
        ("GIT_CONFIG_VALUE_0", hooks_s.as_str()),
    ];
    let out = f.run_sync("broken", "", &config);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).contains(&format!("ERROR: {}: ", f.path("broken"))),
        "{}",
        text(&out.stderr)
    );
    assert!(!text(&out.stdout).contains("KEPT:"));
    assert!(f.w["broken"].read("a.md").contains("確かめの追記"));
}

#[test]
fn a_malformed_worktree_state_stops() {
    let f = fixture();
    let workdir = f.root.join("bad-state");
    std::fs::create_dir_all(&workdir).unwrap();
    std::fs::copy(f.workdir.join("commit-map"), workdir.join("commit-map")).unwrap();
    std::fs::write(workdir.join("worktree-state"), "only-one-column\n").unwrap();
    let out = history_rewrite(
        &f.root,
        &[
            "sync-worktrees",
            "--workdir",
            &workdir.display().to_string(),
        ],
        &[],
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("タブで区切った 3 列でない"),
        "{}",
        text(&out.stderr)
    );
}
