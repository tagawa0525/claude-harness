//! `claude-hooks user-prompt-submit`: UserPromptSubmit の hook。作業ディレクトリのリポジトリの既定ブランチ（手元の
//! `refs/heads/<既定>` と origin の追跡ブランチ `refs/remotes/origin/<既定>`）の先端が、同じセッションの前のプロンプトの
//! 時点から進んでいたら、新しいコミットの件名を標準出力に出す（標準出力はモデルの文脈に足される。#363）。
//!
//! - 前の先端は、clone の共通の git ディレクトリの [`STATE_DIR`] に、セッションの ID の名前のファイルで覚える（セッションと
//!   clone の組ごと。同じ clone の worktree は同じ組）。初めてのプロンプトは覚えるだけで、何も出さない。先端を書き直すとき、
//!   [`STALE_AFTER`] を超えて書き直していない記録を消す（終わったセッションの記録が clone に溜まらないように）
//! - fetch はしない（プロンプトのたびの fetch は遅い。同じ機械のほかのセッションは同じ clone の ref を進める）
//! - 件名は first-parent で新しい順に [`MAX_SUBJECTS`] 件まで、超えた分は「ほか N 件」の 1 行
//! - git のリポジトリの外、既定ブランチが分からない、git の失敗では何も出さない。記録を書けない理由は標準エラーに出す
//!   （文脈に出すと、書けない間はプロンプトのたびに同じ行が入る）。終了コードは常に 0（UserPromptSubmit の 2 はプロンプトを
//!   止める）。プロンプトのたびに走るので、git の呼び出しは 4 回まで

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

/// 前の先端の置き場（共通の git ディレクトリの下）
const STATE_DIR: &str = "claude-prompt-heads";
/// 出す件名の数
const MAX_SUBJECTS: usize = 5;
/// これより長く書き直していない記録は、終わったセッションのものとみなして消す。消した後に戻ったセッションは、次の
/// プロンプトを初めてのプロンプトとして覚え直す（その 1 回の進みを知らせないだけ）
const STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 60 * 60);
/// 先端が動かないプロンプトで、記録の更新の時刻をこれより古ければ今にする（動かないリポジトリで続くセッションの記録を
/// [`STALE_AFTER`] で消さないため。プロンプトごとには書かない）
const REFRESH_AFTER: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

pub fn main() -> i32 {
    let mut raw = String::new();
    let Some(input) = std::io::stdin()
        .read_to_string(&mut raw)
        .ok()
        .and_then(|_| crate::input::Input::parse(&raw))
    else {
        return 0;
    };
    if let Some(text) = notice(&input.cwd, &input.session_id) {
        print!("{text}");
    }
    0
}

/// dir のリポジトリの既定ブランチが、session の前のプロンプトから進んでいれば、その知らせ
fn notice(dir: &Path, session: &str) -> Option<String> {
    // セッションの ID はファイルの名前にする。パスを作れる文字を含むものは覚えない
    if session.is_empty()
        || !session
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    if crate::git::not_a_directory(dir) {
        return None;
    }
    let common = git(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let tips = default_tips(dir)?;
    let state = Path::new(&common).join(STATE_DIR).join(session);
    let previous = read_tips(&state);
    if previous.as_ref() == Some(&tips) {
        keep_fresh(&state);
        return None;
    }
    if let Err(e) = write_tips(&state, &tips) {
        eprintln!(
            "claude-hooks: 既定ブランチの先端を {} に覚えられません（書けるまで進みを知らせません）: {e}",
            state.display()
        );
        return None;
    }
    remove_stale(&state);
    let previous = previous?;
    let subjects = new_subjects(dir, &tips, &previous)?;
    (!subjects.is_empty()).then(|| render(&tips, &previous, &subjects))
}

/// ref の完全な名前 → 先端のオブジェクトの名前
type Tips = BTreeMap<String, String>;

/// 既定ブランチの手元と origin の追跡ブランチのうち、あるものの先端。既定ブランチは [`crate::git::default_branch`] と同じ順（origin/HEAD → origin/main → origin/master → main → master）
/// で決めるが、git の呼び出しを 1 回（origin/HEAD が main と master のほかを指すときは 2 回）にまとめる
fn default_tips(dir: &Path) -> Option<Tips> {
    const ORIGIN_HEAD: &str = "refs/remotes/origin/HEAD";
    const CANDIDATES: [&str; 4] = [
        "refs/remotes/origin/main",
        "refs/remotes/origin/master",
        "refs/heads/main",
        "refs/heads/master",
    ];
    let mut patterns = vec![ORIGIN_HEAD];
    patterns.extend(CANDIDATES);
    let found = refs(dir, &patterns)?;
    let name = match found.get(ORIGIN_HEAD) {
        Some((_, target)) if !target.is_empty() => {
            target.strip_prefix("refs/remotes/origin/")?.to_string()
        }
        _ => {
            let first = CANDIDATES.iter().find(|r| found.contains_key(**r))?;
            first.rsplit('/').next()?.to_string()
        }
    };
    let wanted = [
        format!("refs/heads/{name}"),
        format!("refs/remotes/origin/{name}"),
    ];
    let found = if wanted.iter().all(|r| CANDIDATES.contains(&r.as_str())) {
        found
    } else {
        refs(dir, &[wanted[0].as_str(), wanted[1].as_str()])?
    };
    let tips: Tips = wanted
        .iter()
        .filter_map(|r| found.get(r).map(|(oid, _)| (r.clone(), oid.clone())))
        .collect();
    (!tips.is_empty()).then_some(tips)
}

/// patterns に名前がちょうど一致する ref の、オブジェクトの名前と symbolic ref の先（無ければ空）。for-each-ref の
/// パターンは `/` の区切りまでの前方一致なので、ちょうど一致するものだけを残す
fn refs(dir: &Path, patterns: &[&str]) -> Option<BTreeMap<String, (String, String)>> {
    let mut args = vec![
        "for-each-ref",
        "--format=%(refname)%00%(objectname)%00%(symref)",
    ];
    args.extend(patterns);
    let out = git(dir, &args)?;
    Some(
        out.lines()
            .filter_map(|line| {
                let mut fields = line.split('\0');
                let (name, oid, target) = (fields.next()?, fields.next()?, fields.next()?);
                patterns
                    .contains(&name)
                    .then(|| (name.to_string(), (oid.to_string(), target.to_string())))
            })
            .collect(),
    )
}

/// 前の先端からの新しいコミット（first-parent、新しい順に `<短い名前> <件名>`）。前の先端のオブジェクトが無い（gc で
/// 消えた）など、git が失敗すれば None
fn new_subjects(dir: &Path, tips: &Tips, previous: &Tips) -> Option<Vec<String>> {
    let mut args = vec![
        "log".to_string(),
        "--first-parent".to_string(),
        "--format=%h %s".to_string(),
    ];
    args.extend(tips.values().cloned());
    args.extend(previous.values().map(|oid| format!("^{oid}")));
    args.push("--".to_string());
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = git(dir, &args)?;
    Some(out.lines().map(str::to_string).collect())
}

/// 知らせ。見出しには、前の先端から動いた ref だけを書く
fn render(tips: &Tips, previous: &Tips, subjects: &[String]) -> String {
    let names: Vec<&str> = tips
        .iter()
        .filter(|(r, oid)| previous.get(*r) != Some(*oid))
        .map(|(r, _)| {
            r.strip_prefix("refs/remotes/")
                .or_else(|| r.strip_prefix("refs/heads/"))
                .unwrap_or(r)
        })
        .collect();
    let mut text = format!(
        "既定ブランチ（{}）が、前のプロンプトの後に進みました（ほかのセッションのマージかもしれません）:\n",
        names.join("、")
    );
    for subject in subjects.iter().take(MAX_SUBJECTS) {
        text.push_str(&format!("- {subject}\n"));
    }
    if subjects.len() > MAX_SUBJECTS {
        text.push_str(&format!("- ほか {} 件\n", subjects.len() - MAX_SUBJECTS));
    }
    text
}

/// 覚えた先端（1 行に `<ref> <オブジェクトの名前>`）。ファイルが無いか読めないか、読める行が無ければ None（初めての
/// プロンプト）。空の記録を前の先端にすると、除く先端の無い `git log` が履歴のすべてを新しいコミットとして出す
fn read_tips(path: &Path) -> Option<Tips> {
    let text = std::fs::read_to_string(path).ok()?;
    let tips: Tips = text
        .lines()
        .filter_map(|l| l.split_once(' '))
        .map(|(r, oid)| (r.to_string(), oid.to_string()))
        .collect();
    (!tips.is_empty()).then_some(tips)
}

/// 隣の一時のファイルに書いてから名前を付け替える（途中で止まっても、前の記録か新しい記録のどちらかが残る）
fn write_tips(path: &Path, tips: &Tips) -> std::io::Result<()> {
    let dir = path.parent().expect("記録のパスは置き場の下");
    std::fs::create_dir_all(dir)?;
    let text: String = tips.iter().map(|(r, oid)| format!("{r} {oid}\n")).collect();
    let mut name = path
        .file_name()
        .expect("記録のパスはファイルの名前で終わる")
        .to_os_string();
    name.push(".tmp");
    let tmp = dir.join(name);
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// 記録の更新の時刻が [`REFRESH_AFTER`] より古ければ今にする。補助なので、失敗は無視する
fn keep_fresh(path: &Path) {
    let now = std::time::SystemTime::now();
    let old = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| now.duration_since(t).ok())
        .is_some_and(|age| age > REFRESH_AFTER);
    if old && let Ok(file) = std::fs::File::options().write(true).open(path) {
        let _ = file.set_modified(now);
    }
}

/// keep の隣の記録のうち、[`STALE_AFTER`] を超えて書き直していないものを消す。片付けは補助なので、失敗は無視する
fn remove_stale(keep: &Path) {
    let Some(dir) = keep.parent() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age > STALE_AFTER);
        if stale && path != keep {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// `git <args>` を dir で実行し、成功なら標準出力（末尾の改行を除く）。リポジトリを指す GIT_* を継承しない（git の hook
/// から起動したセッションでは GIT_DIR が渡り、dir の clone でなくそのリポジトリを引く）。起動できなくても理由を出さない
/// （[`crate::git::run`] と違い、知らせは補助で、理由を出す置き場が無い）
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_INDEX_FILE")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then(|| {
        String::from_utf8_lossy(&out.stdout)
            .trim_end_matches('\n')
            .to_string()
    })
}
