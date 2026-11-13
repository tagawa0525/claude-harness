//! `claude-hooks git-hook staged-checks [nix|python|markdown|rust]...`: ステージした版（コミットに入る中身）の Nix、
//! Python、Markdown、Rust の書式と lint を検査する（ADR-0028、ADR-0086）。グローバルの pre-commit がリポジトリの直下で
//! 呼び、run-checks.sh の Markdown の段が `staged-checks markdown` で呼ぶ。ツール（nixfmt、ruff、markdownlint、
//! rustfmt と cargo）は利用者の PATH のものを使い、無ければその言語を飛ばす。言語を渡せばその言語だけを検査する。
//!
//! 作業ツリーは書き換えない。一部だけステージしたファイルで、ステージしていない編集を検査せず、コミットに入れない
//! ため。Markdown の自動修正はステージした版に当ててステージし直し、作業ツリーのファイルがステージした版と同じ
//! ときだけ、作業ツリーにも書く。
//!
//! 終了コード: 0 = 違反が無い、1 = 違反がある、2 = 使い方の誤り

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use crate::process::output_with_input;

const LANGUAGES: [&str; 4] = ["nix", "python", "markdown", "rust"];

/// ステージした普通のファイル（symlink とサブモジュールは除く）
struct Staged {
    path: PathBuf,
    blob: String,
    mode: String,
}

/// 一時の作業の場所（終わったら消す）
struct Work(PathBuf);

impl Drop for Work {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn run(langs: &[String]) -> i32 {
    if let Some(lang) = langs.iter().find(|l| !LANGUAGES.contains(&l.as_str())) {
        eprintln!("staged-checks: 知らない言語です: {lang}（nix、python、markdown、rust）");
        return 2;
    }
    let wants = |lang: &str| langs.is_empty() || langs.iter().any(|l| l == lang);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let work = Work(std::env::temp_dir().join(format!(
        "claude-hooks-staged-{}-{nanos}",
        std::process::id()
    )));
    if let Err(e) = std::fs::create_dir_all(&work.0) {
        println!("❌ 一時のディレクトリを作れません（{e}）");
        return 1;
    }
    let mut failed = false;
    for (lang, spec, check) in [
        ("nix", "*.nix", nix as fn(&[Staged], &Path) -> bool),
        ("python", "*.py", python),
        ("markdown", "*.md", markdown),
        ("rust", "*.rs", rust),
    ] {
        if !wants(lang) {
            continue;
        }
        match collect(spec) {
            Ok(files) if files.is_empty() => {}
            Ok(files) => failed |= !check(&files, &work.0),
            Err(why) => {
                println!("❌ ステージしたファイルを読めません（{why}）");
                failed = true;
            }
        }
    }
    i32::from(failed)
}

/// git が出したパスのバイト列（Unix はバイト列のまま。UTF-8 とは限らない）
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
    }
}

/// pathspec に当たる、ステージした普通のファイル。`--raw -z` の読み方は ja-terms の src/staged.rs と同じ
fn collect(spec: &str) -> Result<Vec<Staged>, String> {
    let out = git(
        &[
            "diff",
            "--cached",
            "--raw",
            "-z",
            "--no-abbrev",
            "--no-renames",
            "--diff-filter=ACMRT",
            "--",
            spec,
        ],
        None,
    )?;
    let mut files = Vec::new();
    let mut fields = out.split(|b| *b == 0).filter(|f| !f.is_empty());
    while let (Some(meta), Some(path)) = (fields.next(), fields.next()) {
        let meta = String::from_utf8_lossy(meta);
        // `:<旧の mode> <新の mode> <旧のブロブ> <新のブロブ> <状態>`
        let parts: Vec<&str> = meta.split_whitespace().collect();
        let (Some(mode), Some(blob)) = (parts.get(1), parts.get(3)) else {
            continue;
        };
        if matches!(*mode, "120000" | "160000") {
            continue;
        }
        files.push(Staged {
            path: path_from_bytes(path),
            blob: blob.to_string(),
            mode: mode.to_string(),
        });
    }
    Ok(files)
}

/// リポジトリの直下で git を実行し、成功なら標準出力を返す
fn git(args: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>, String> {
    match crate::git::run(Path::new("."), args, input) {
        Some(out) if out.status.success() => Ok(out.stdout),
        Some(out) => Err(format!(
            "git {} が失敗しました: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        None => Err(format!("git {} を起動できません", args.join(" "))),
    }
}

fn blob(staged: &Staged) -> Result<Vec<u8>, String> {
    git(&["cat-file", "blob", &staged.blob], None)
}

/// PATH で name を探す。Windows は PATHEXT の拡張子（npm の markdownlint.cmd など）も探し、見つけたパスで起動する
/// （Command は拡張子の無い名前に .exe しか足さない）
fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
            .split(';')
            .filter(|e| !e.is_empty())
            .map(str::to_string)
            .collect()
    } else {
        vec![String::new()]
    };
    std::env::split_paths(&path).find_map(|dir| {
        exts.iter()
            .map(|ext| dir.join(format!("{name}{ext}")))
            .find(|p| p.is_file())
    })
}

/// tool に input を渡して実行する（stdout と stderr は捨てるか、受けるか、そのまま見せる）
fn pipe(
    tool: &Path,
    args: &[&std::ffi::OsStr],
    input: &[u8],
    cwd: &Path,
    show: bool,
) -> Option<Output> {
    let mut cmd = Command::new(tool);
    cmd.args(args).current_dir(cwd);
    if show {
        cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    } else {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    }
    output_with_input(&mut cmd, input).ok()
}

fn ok(out: &Option<Output>) -> bool {
    out.as_ref().is_some_and(|o| o.status.success())
}

fn names(files: &[&PathBuf]) -> String {
    files
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

fn nix(files: &[Staged], _work: &Path) -> bool {
    let Some(nixfmt) = find_on_path("nixfmt") else {
        println!("SKIP: Nix の検査（nixfmt が PATH に無い）");
        return true;
    };
    println!("🔍 Checking Nix format...");
    let mut bad = Vec::new();
    for f in files {
        let passed = blob(f).is_ok_and(|text| {
            ok(&pipe(
                &nixfmt,
                &["--check".as_ref(), "-".as_ref()],
                &text,
                Path::new("."),
                false,
            ))
        });
        if !passed {
            bad.push(&f.path);
        }
    }
    if bad.is_empty() {
        return true;
    }
    println!(
        "❌ Nix format check failed: {}. Run: nixfmt <files>",
        names(&bad)
    );
    false
}

/// `--stdin-filename` の場所から、ruff はプロジェクトの設定を探す
fn python(files: &[Staged], _work: &Path) -> bool {
    let Some(ruff) = find_on_path("ruff") else {
        println!("SKIP: Python の検査（ruff が PATH に無い）");
        return true;
    };
    let texts: Vec<Option<Vec<u8>>> = files.iter().map(|f| blob(f).ok()).collect();
    println!("🔍 Checking Python format...");
    let mut passed = true;
    let mut bad = Vec::new();
    for (f, text) in files.iter().zip(&texts) {
        let formatted = text.as_ref().is_some_and(|text| {
            let args = [
                "format".as_ref(),
                "--check".as_ref(),
                "--stdin-filename".as_ref(),
                f.path.as_os_str(),
                "-".as_ref(),
            ];
            ok(&pipe(&ruff, &args, text, Path::new("."), false))
        });
        if !formatted {
            bad.push(&f.path);
        }
    }
    if !bad.is_empty() {
        println!(
            "❌ Python format check failed: {}. Run: ruff format <files>",
            names(&bad)
        );
        passed = false;
    }
    println!("🔍 Checking Python lint...");
    let mut lint_failed = false;
    for (f, text) in files.iter().zip(&texts) {
        let linted = text.as_ref().is_some_and(|text| {
            let args = [
                "check".as_ref(),
                "--quiet".as_ref(),
                "--stdin-filename".as_ref(),
                f.path.as_os_str(),
                "-".as_ref(),
            ];
            ok(&pipe(&ruff, &args, text, Path::new("."), true))
        });
        lint_failed |= !linted;
    }
    if lint_failed {
        println!("❌ Python lint failed. Run: ruff check --fix <files>");
        passed = false;
    }
    passed
}

/// ステージした版を元のパスのまま一時の場所に書き出し、そこで markdownlint を実行する（直下の設定と
/// .markdownlintignore も、ステージした版をコピーし、相対のパスの書き方をそのまま効かせる。設定の extends が指す
/// 別のファイルはコピーしない）
fn markdown(files: &[Staged], work: &Path) -> bool {
    let Some(markdownlint) = find_on_path("markdownlint") else {
        println!("SKIP: Markdown の検査（markdownlint が PATH に無い）");
        return true;
    };
    println!("🔧 Auto-fixing Markdown lint...");
    let md = work.join("md");
    let mut passed = true;
    for name in [
        ".markdownlint.json",
        ".markdownlint.jsonc",
        ".markdownlint.yaml",
        ".markdownlint.yml",
        ".markdownlintrc",
        ".markdownlintignore",
    ] {
        if let Ok(text) = git(&["cat-file", "blob", &format!(":{name}")], None) {
            let _ = std::fs::create_dir_all(&md);
            if std::fs::write(md.join(name), text).is_err() {
                passed = false;
            }
        }
    }
    for f in files {
        let dest = md.join(&f.path);
        let written = blob(f).and_then(|text| {
            std::fs::create_dir_all(dest.parent().unwrap_or(&md))
                .and_then(|()| std::fs::write(&dest, text))
                .map_err(|e| e.to_string())
        });
        if let Err(why) = written {
            println!("❌ {} を書き出せません（{why}）", f.path.display());
            return false;
        }
    }
    let paths: Vec<&std::ffi::OsStr> = files.iter().map(|f| f.path.as_os_str()).collect();
    // MD038（code span の内側の空白）だけは自動修正から外し、続く検査で違反として出す。markdownlint 0.50.0 の
    // --fix は code span の先頭の空白を落とし（` X` が `X` になる）、空白に意味のある書き方（シェルの <<' X' など）を
    // 書き手が気づかないまま変えるため。設定で MD038 を無効にしないのは、検査まで止めて違反に気づけなくなるため
    let _ = Command::new(&markdownlint)
        .args(["--fix", "--disable", "MD038", "--"])
        .args(&paths)
        .current_dir(&md)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    // markdownlint --fix が直せない MD040（言語指定なし）/ MD060（CJK テーブル整列）を補完する。実体は
    // language-checks スキルの同期先（~/.claude）。無ければ飛ばし、続く検査で残った違反として出る
    let fixer =
        crate::shell::home().join(".claude/skills/language-checks/scripts/fix-markdown-lint.py");
    match (fixer.is_file(), find_on_path("python3")) {
        (true, Some(python)) => {
            // fix-markdown-lint.py は -- で始まる引数を飛ばすので、./ を付けて渡す
            let fixed = Command::new(python)
                .arg(&fixer)
                .args(files.iter().map(|f| Path::new(".").join(&f.path)))
                .current_dir(&md)
                .status()
                .is_ok_and(|s| s.success());
            if !fixed {
                println!(
                    "❌ fix-markdown-lint.py failed. Run: python3 {} <files>",
                    fixer.display()
                );
                passed = false;
            }
        }
        _ => println!(
            "SKIP: fix-markdown-lint.py（{} か python3 が無い）",
            fixer.display()
        ),
    }
    for f in files {
        if let Err(why) = restage(f, &md) {
            println!(
                "❌ 自動修正した {} をステージし直せません（{why}）",
                f.path.display()
            );
            passed = false;
        }
    }
    println!("🔍 Checking Markdown lint...");
    let checked = Command::new(&markdownlint)
        .arg("--")
        .args(&paths)
        .current_dir(&md)
        .status()
        .is_ok_and(|s| s.success());
    if !checked {
        println!("❌ Markdown lint failed (unfixable issues remain)");
        passed = false;
    }
    passed
}

/// 自動修正した版をステージし直し、ステージしていない編集の無い普通のファイルだけ、作業ツリーにも書く（あれば、
/// 作業ツリーは触らない）。作業ツリーのファイルは改行などの変換を経た形なので、元のブロブを同じ変換で作業ツリーの
/// 形にして、バイト列で比べる（作業ツリーをブロブにして比べると、clean のフィルタが落とす作業ツリーだけの中身を
/// 見逃す）。symlink に置き換わっていれば、その先（リポジトリの外かもしれない）を書き換えない。変換が済んでから書き、
/// 途中で空にしない
fn restage(f: &Staged, md: &Path) -> Result<(), String> {
    let copy = md.join(&f.path);
    let copy = copy.to_string_lossy();
    // 書き出したのはブロブ（clean の後の形）なので、clean のフィルタを二度掛けない
    let fixed = git(&["hash-object", "-w", "--no-filters", copy.as_ref()], None)?;
    let fixed = String::from_utf8_lossy(&fixed).trim().to_string();
    if fixed == f.blob {
        return Ok(());
    }
    let path = f.path.to_string_lossy();
    git(
        &[
            "update-index",
            "--cacheinfo",
            &format!("{},{fixed},{path}", f.mode),
        ],
        None,
    )?;
    let plain = std::fs::symlink_metadata(&f.path).is_ok_and(|m| m.file_type().is_file());
    if !plain {
        return Ok(());
    }
    let filters = format!("--path={path}");
    let before = git(&["cat-file", "--filters", &filters, &f.blob], None)?;
    if std::fs::read(&f.path).ok().as_deref() != Some(before.as_slice()) {
        return Ok(());
    }
    let after = git(&["cat-file", "--filters", &filters, &fixed], None)?;
    std::fs::write(&f.path, after).map_err(|e| e.to_string())
}

/// 1 つずつ標準入力で rustfmt に渡し（ファイルで渡すと mod の先を読みにいく）、整えた出力と比べる（標準入力では
/// --check が差を見ない）。edition は最寄りの Cargo.toml のパッケージのもの（cargo metadata が workspace からの継承
/// も解く）。パッケージの無い Cargo.toml（virtual workspace）の下とクレートの外は 2024。rustfmt.toml は、ファイルの
/// ディレクトリから探させる（標準入力では rustfmt.toml の ignore が効かない）
fn rust(files: &[Staged], _work: &Path) -> bool {
    let Some(rustfmt) = find_on_path("rustfmt") else {
        println!("SKIP: Rust の検査（rustfmt が PATH に無い）");
        return true;
    };
    println!("🔍 Checking Rust format...");
    let cargo = find_on_path("cargo");
    // manifest ごとの edition。cargo metadata が失敗した manifest は None を入れ、1 回だけ報告する
    let mut editions: HashMap<PathBuf, Option<String>> = HashMap::new();
    let mut passed = true;
    let mut bad = Vec::new();
    for f in files {
        let mut edition = None;
        if let Some(manifest) = manifest_of(&f.path) {
            let Some(cargo) = &cargo else {
                continue;
            };
            let found = editions.entry(manifest.clone()).or_insert_with(|| {
                match package_edition(cargo, &manifest) {
                    Ok(ed) => Some(ed.unwrap_or_default()),
                    Err(why) => {
                        println!(
                            "❌ cargo metadata が失敗しました（{}）:\n{why}",
                            manifest.display()
                        );
                        passed = false;
                        None
                    }
                }
            });
            let Some(found) = found else {
                continue;
            };
            edition = Some(found.clone()).filter(|e| !e.is_empty());
        }
        let Ok(text) = blob(f) else {
            passed = false;
            continue;
        };
        let dir = f
            .path
            .parent()
            .filter(|d| d.is_dir() && !d.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let edition = edition.unwrap_or_else(|| "2024".to_string());
        let out = pipe(
            &rustfmt,
            &["--edition".as_ref(), edition.as_ref()],
            &text,
            dir,
            false,
        );
        match out {
            Some(o) if o.status.success() => {
                if o.stdout != text {
                    bad.push(&f.path);
                }
            }
            o => {
                println!(
                    "❌ rustfmt が {} を読めません:\n{}",
                    f.path.display(),
                    o.map(|o| String::from_utf8_lossy(&o.stderr).into_owned())
                        .unwrap_or_default()
                );
                passed = false;
            }
        }
    }
    if !bad.is_empty() {
        println!(
            "❌ Rust format check failed: {}. Run: cargo fmt（クレートの外は rustfmt --edition 2024 <files>）",
            names(&bad)
        );
        passed = false;
    }
    passed
}

/// path の最寄りの Cargo.toml（作業ツリーで、ファイルのディレクトリからリポジトリの直下まで上に探す）
fn manifest_of(path: &Path) -> Option<PathBuf> {
    let mut dir = path.parent();
    while let Some(d) = dir {
        let manifest = if d.as_os_str().is_empty() {
            PathBuf::from("Cargo.toml")
        } else {
            d.join("Cargo.toml")
        };
        if manifest.is_file() {
            return Some(manifest);
        }
        dir = d.parent();
    }
    None
}

/// manifest のパッケージの edition（パッケージの無い virtual workspace は None）
fn package_edition(cargo: &Path, manifest: &Path) -> Result<Option<String>, String> {
    let out = Command::new(cargo)
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--manifest-path",
        ])
        .arg(manifest)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    let metadata: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("出力を JSON として読めません（{e}）"))?;
    let want = comparable(&std::fs::canonicalize(manifest).map_err(|e| e.to_string())?);
    Ok(metadata["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|p| {
            p["manifest_path"]
                .as_str()
                .is_some_and(|m| comparable(Path::new(m)) == want)
        })
        .and_then(|p| p["edition"].as_str())
        .map(str::to_string))
}

/// 比べるためのパス（Windows の canonicalize が付ける `\\?\` を外す。cargo metadata は付けない）
fn comparable(path: &Path) -> String {
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\")
        .map(str::to_string)
        .unwrap_or(text)
}
