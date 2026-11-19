//! `claude-hooks user-prompt-submit`: UserPromptSubmit の hook。作業ディレクトリのリポジトリの既定ブランチ（手元の
//! `refs/heads/<既定>` と origin の追跡ブランチ `refs/remotes/origin/<既定>`）の先端が、同じセッションの前のプロンプトの
//! 時点から進んでいたら、新しいコミットの件名を標準出力に出す（標準出力はモデルの文脈に足される。#363）。
//!
//! - 前の先端は、clone の共通の git ディレクトリの [`STATE_DIR`] に、セッションの ID の名前のファイルで覚える（セッションと
//!   clone の組ごと。同じ clone の worktree は同じ組）。初めてのプロンプトは覚えるだけで、何も出さない
//! - fetch はしない（プロンプトのたびの fetch は遅い。同じ機械のほかのセッションは同じ clone の ref を進める）
//! - 件名は first-parent で新しい順に [`MAX_SUBJECTS`] 件まで、超えた分は「ほか N 件」の 1 行
//! - git のリポジトリの外、既定ブランチが分からない、git の失敗では何も出さない。終了コードは常に 0（UserPromptSubmit の
//!   2 はプロンプトを止める）。プロンプトのたびに走るので、git の呼び出しは 4 回まで

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

/// 前の先端の置き場（共通の git ディレクトリの下）
const STATE_DIR: &str = "claude-prompt-heads";
/// 出す件名の数
const MAX_SUBJECTS: usize = 5;

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
        return None;
    }
    if let Err(e) = write_tips(&state, &tips) {
        return Some(format!(
            "claude-hooks: 既定ブランチの先端を {} に覚えられません（次のプロンプトで進みを知らせられません）: {e}\n",
            state.display()
        ));
    }
    let previous = previous?;
    let subjects = new_subjects(dir, &tips, &previous)?;
    (!subjects.is_empty()).then(|| render(&tips, &subjects))
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

fn render(tips: &Tips, subjects: &[String]) -> String {
    let names: Vec<&str> = tips
        .keys()
        .map(|r| {
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

/// 覚えた先端（1 行に `<ref> <オブジェクトの名前>`）。ファイルが無いか読めなければ None（初めてのプロンプト）
fn read_tips(path: &Path) -> Option<Tips> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(
        text.lines()
            .filter_map(|l| l.split_once(' '))
            .map(|(r, oid)| (r.to_string(), oid.to_string()))
            .collect(),
    )
}

fn write_tips(path: &Path, tips: &Tips) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text: String = tips.iter().map(|(r, oid)| format!("{r} {oid}\n")).collect();
    std::fs::write(path, text)
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
