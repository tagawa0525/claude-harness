//! `run-checks.sh`: プロジェクトの言語を検出し、フォーマット → リント → テストを実行する（language-checks のスキルの
//! 規約）。使い方と出力は run-checks.sh の冒頭に書く。
//!
//! 言語の検出（いずれかを満たせば対象）:
//! - Rust: Cargo.toml がある、または .rs がステージ済み。ルートに Cargo.toml が無ければ、ステージ済みの .rs から最も
//!   近い Cargo.toml のディレクトリ（crate）ごとに実行する。ルートに Cargo.toml と scripts/run-rust-docs-tests.sh
//!   （実行権限が要る）があれば、差分が .md だけなら fmt、clippy、cargo test の代わりにそのスクリプトだけを、ほかの
//!   差分では全体の後にそのスクリプトも実行する
//! - Python: pyproject.toml / setup.py / requirements.txt がある、または .py がステージ済み。テストは
//!   scripts/run-python-tests.sh があればそれ、なければ tests/ かルートの test_*.py を pytest で、それも無ければ追跡
//!   している test_*.py / *_test.py のあるディレクトリごとに
//! - Nix: flake.nix がある、または .nix がステージ済み
//! - Markdown: .md がステージ済み（staged-checks markdown で、ステージした版を直して検査する。--merge では検査だけ）。
//!   .pre-commit-config.yaml で design-records にオプトインしていれば、.md の変更が無くても design-records check
//! - Shell: .sh がステージ済み（ステージ済みのファイルだけを shellcheck で検査）
//!
//! 「ステージ済み」は、--merge では HEAD と作業ツリーの差分、--base では分岐点と作業ツリーの差分に読み替える。
//! 最初に失敗したチェックで止まる。ツールが PATH に無ければ SKIP を出して続ける。`RUN:` と `FAILED:` の行のコマンドは、
//! sh の版の段の関数の名前（`cargo_in`、`nix_fmt_check` など）をそのまま出す（読む側の文と揃える）

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::staged_checks::{executable, find_on_path};

const USAGE: &str = "Usage: run-checks.sh [-C <dir>] [--merge] [--base <ref>]";

/// 文書の差分で実行するテストの宣言。文書を読む Rust のテスト（文書どうしの整合など）を持つリポジトリが置き、
/// 差分が .md だけのときは fmt、clippy、全体の cargo test の代わりにこれを実行する（scripts/run-python-tests.sh と同じ形）。
/// ほかの差分では全体の後にも実行する。宣言の外の変更（Cargo.toml のパッケージ名など）で壊れた宣言が、次の文書だけの
/// 差分で初めて落ちないようにするため。同じ理由で、実行権限も差分によらず確かめる
const RUST_DOCS_TESTS: &str = "scripts/run-rust-docs-tests.sh";
const PYTHON_TESTS: &str = "scripts/run-python-tests.sh";

/// 失敗で止まるときの終了コード（理由は出力済み）
type Step = Result<(), i32>;

struct Checks {
    /// HEAD（--merge）か分岐点（--base）と作業ツリーの差分で選ぶ。偽ならステージ済みの差分
    merge: bool,
    diff_base: String,
    /// 排他の Slurm のジョブ中の制限（ADR-0041）。cargo を包むコマンド、cargo の -j、nix flake check の引数
    cargo_pin: Vec<String>,
    cargo_jobs: Vec<String>,
    nix_limit: Vec<String>,
}

pub fn main(args: &[String]) -> i32 {
    let mut merge = false;
    let mut base_ref = None;
    let mut dir = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--merge" => merge = true,
            "--base" | "-C" => {
                let Some(value) = rest.next() else {
                    eprintln!("ERROR: {arg} には引数が要ります");
                    return 2;
                };
                if arg == "-C" {
                    dir = Some(value.clone());
                } else {
                    base_ref = Some(value.clone());
                    merge = true;
                }
            }
            _ => {
                eprintln!("ERROR: 知らないオプション: {arg}（{USAGE}）");
                return 2;
            }
        }
    }
    if let Some(dir) = &dir
        && std::env::set_current_dir(dir).is_err()
    {
        eprintln!("ERROR: -C {dir} に移れません");
        return 2;
    }
    let Some(root) = git_text(&["rev-parse", "--show-toplevel"]) else {
        eprintln!("ERROR: git リポジトリ内で実行してください");
        return 1;
    };
    if std::env::set_current_dir(&root).is_err() {
        return 1;
    }
    let diff_base = match &base_ref {
        None => "HEAD".to_string(),
        Some(base) => match git_text(&["merge-base", base, "HEAD"]) {
            Some(sha) => sha,
            None => {
                eprintln!("ERROR: --base {base} と HEAD の分岐点を求められません");
                return 2;
            }
        },
    };
    let mut checks = Checks {
        merge,
        diff_base,
        cargo_pin: Vec::new(),
        cargo_jobs: Vec::new(),
        nix_limit: Vec::new(),
    };
    checks.limit_during_exclusive_job();
    match checks.run() {
        Ok(()) => 0,
        Err(code) => code,
    }
}

/// `git <args>` の標準出力（末尾の改行を除く）。失敗すれば None。標準エラーは捨てる（失敗は呼ぶ側が説明する）
fn git_text(args: &[&str]) -> Option<String> {
    git_output(args, Stdio::null()).map(|out| {
        String::from_utf8_lossy(&out)
            .trim_end_matches('\n')
            .to_string()
    })
}

/// `git <args>` の標準出力。失敗すれば None。標準エラーは引き継ぐ
fn git_bytes(args: &[&str]) -> Option<Vec<u8>> {
    git_output(args, Stdio::inherit())
}

fn git_output(args: &[&str], stderr: Stdio) -> Option<Vec<u8>> {
    Command::new("git")
        .args(args)
        .stdin(Stdio::null())
        .stderr(stderr)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| o.stdout)
}

/// -z で区切ったパスの一覧
fn nul_paths(bytes: &[u8]) -> Vec<PathBuf> {
    bytes
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(path_of)
        .collect()
}

#[cfg(unix)]
fn path_of(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(OsString::from_vec(bytes.to_vec()))
}

/// git は Windows でもパスを UTF-8 で出す
#[cfg(not(unix))]
fn path_of(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

/// bash の `printf %q` と同じ引用（修正コマンドと RUN の行を、コピーしてそのまま実行できるようにする）
fn shell_quote(text: &str) -> String {
    if text.is_empty() {
        return "''".to_string();
    }
    if text.chars().any(|c| c.is_control()) {
        let mut out = String::from("$'");
        for c in text.chars() {
            match c {
                '\n' => out.push_str("\\n"),
                '\t' => out.push_str("\\t"),
                '\r' => out.push_str("\\r"),
                '\\' => out.push_str("\\\\"),
                '\'' => out.push_str("\\'"),
                c if c.is_control() => out.push_str(&format!("\\{:03o}", c as u32)),
                c => out.push(c),
            }
        }
        out.push('\'');
        return out;
    }
    let mut out = String::new();
    for (i, c) in text.chars().enumerate() {
        let plain = !c.is_ascii()
            || c.is_ascii_alphanumeric()
            || "%+-./:=@_".contains(c)
            || (i > 0 && "#~".contains(c));
        if !plain {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// コマンドを起動して終了を待つ（標準入出力は引き継ぐ）。名前は PATH で探したパスで起動する（Windows の
/// markdownlint.cmd などを見つけるため）。起動できなければ理由を標準エラーに出して失敗とする
fn spawn(argv: &[OsString], dir: Option<&Path>) -> bool {
    let Some((program, args)) = argv.split_first() else {
        return false;
    };
    let path = program
        .to_str()
        .and_then(find_on_path)
        .unwrap_or_else(|| PathBuf::from(program));
    let mut cmd = Command::new(&path);
    cmd.args(args);
    if let Some(dir) = dir {
        cmd.current_dir(dir);
    }
    match cmd.status() {
        Ok(status) => status.success(),
        Err(e) => {
            eprintln!(
                "run-checks: {} を起動できません: {e}",
                program.to_string_lossy()
            );
            false
        }
    }
}

fn os(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

/// `xargs -0 -r <prefix>... <files>...` と同じく、files があるときだけ、コマンドの長さの上限（Windows の 32,767 文字）
/// に収まるよう分けて起動する。すべて通れば成功
fn xargs(prefix: &[&str], files: &[PathBuf]) -> bool {
    const LIMIT: usize = 30_000;
    let mut ok = true;
    let mut chunk: Vec<OsString> = Vec::new();
    let mut len = 0;
    for file in files {
        let n = file.as_os_str().len() + 1;
        if !chunk.is_empty() && len + n > LIMIT {
            ok &= spawn(&[os(prefix), std::mem::take(&mut chunk)].concat(), None);
            len = 0;
        }
        chunk.push(file.clone().into_os_string());
        len += n;
    }
    if !chunk.is_empty() {
        ok &= spawn(&[os(prefix), chunk].concat(), None);
    }
    ok
}

fn is_file(path: &str) -> bool {
    Path::new(path).is_file()
}

/// `[[ -x ]]` と同じく、実行の権限があるか
fn is_executable(path: &str) -> bool {
    std::fs::metadata(path).is_ok_and(|m| executable(&m))
}

impl Checks {
    /// 排他の Slurm のジョブが実行中は、そのジョブの計算ノードの上では cargo と nix flake check を 1 コアに限る。全コアを
    /// 使うとそのジョブの時間の記録を乱し、止めるとジョブの間（数時間）マージできないため、止めずに遅く実行する。ほかの
    /// ホストでは手元のコアはノードを乱さないので cargo は限らず、nix のビルドだけをノードに投げずに手元でする
    /// （nixfiles の nix.distributedBuilds。空の --builders でリモートのビルダーを外し、max-jobs = 0 のラップトップでも
    /// 手元のジョブを許す）。claude-hooks の guard-exclusive-slurm は、このスクリプトを対象にせず、判定は同じ関数で行う
    fn limit_during_exclusive_job(&mut self) {
        let Some((name, id, here)) = crate::rules::exclusive_job() else {
            return;
        };
        let job = format!("{name}（{id}）");
        let strings = |args: &[&str]| args.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        if here {
            println!("排他のジョブ {job}がこのホストで実行中なので 1 コアで実行する（遅い）");
            self.cargo_pin = vec![
                "taskset".to_string(),
                "-c".to_string(),
                last_cpu().to_string(),
            ];
            self.cargo_jobs = strings(&["-j", "1"]);
            self.nix_limit = strings(&["--cores", "1", "-j", "1"]);
        } else {
            println!(
                "排他のジョブ {job}が計算ノードで実行中なので、nix のビルドは計算ノードに投げずに手元でビルドする"
            );
            self.nix_limit = strings(&["--builders", "", "--max-jobs", "auto"]);
        }
    }

    /// 検査する差分のファイル名（`git diff --name-only`）。通常はステージ済み、--merge では HEAD と作業ツリーの差分、
    /// --base では分岐点と作業ツリーの差分
    fn diff_names(&self, extra: &[&str]) -> Option<Vec<u8>> {
        let mut args = vec!["diff"];
        if self.merge {
            args.extend([self.diff_base.as_str(), "--name-only"]);
        } else {
            args.extend(["--cached", "--name-only"]);
        }
        args.extend(extra);
        git_bytes(&args)
    }

    /// 検査する変更ファイル（追加、変更、rename の後の名前）。pathspec が空なら全部
    fn changed_files(&self, pathspec: &[&str]) -> Option<Vec<PathBuf>> {
        let mut extra = vec!["--diff-filter=ACMR", "-z"];
        if !pathspec.is_empty() {
            extra.push("--");
            extra.extend(pathspec);
        }
        self.diff_names(&extra).map(|out| nul_paths(&out))
    }

    fn staged_has(&self, pattern: &str) -> bool {
        self.changed_files(&[pattern])
            .is_some_and(|files| !files.is_empty())
    }

    /// 段を 1 つ実行する。tool が PATH に無ければ SKIP。display は RUN と FAILED の行に出すコマンド、run が中身
    fn stage(
        &self,
        lang: &str,
        stage: &str,
        tool: &str,
        fix: &str,
        display: &[String],
        run: impl FnOnce() -> bool,
    ) -> Step {
        if find_on_path(tool).is_none() {
            println!("SKIP: {lang} {stage} ({tool} not found)");
            return Ok(());
        }
        let display = display.join(" ");
        println!("RUN: {lang} {stage}: {display}");
        if run() {
            println!("OK: {lang} {stage}");
            Ok(())
        } else {
            println!("FAILED: {lang} {stage}: {display}");
            if !fix.is_empty() {
                println!("FIX: {fix}");
            }
            Err(1)
        }
    }

    /// argv をそのまま RUN の行に出して実行する段
    fn command_stage(
        &self,
        lang: &str,
        stage: &str,
        tool: &str,
        fix: &str,
        argv: &[String],
    ) -> Step {
        let os_argv: Vec<OsString> = argv.iter().map(OsString::from).collect();
        self.stage(lang, stage, tool, fix, argv, || spawn(&os_argv, None))
    }

    fn run(&self) -> Step {
        let mut detected = false;
        detected |= self.rust()?;
        detected |= self.python()?;
        detected |= self.nix()?;
        detected |= self.markdown()?;
        detected |= self.shell()?;
        if !detected {
            println!(
                "NOTE: 対象言語が見つかりません（Cargo.toml / pyproject.toml / flake.nix、またはステージ済みの .rs / .py / .nix / .md / .sh）"
            );
            return Ok(());
        }
        println!("ALL_OK");
        Ok(())
    }

    // --- Rust ---

    /// crate のディレクトリ dir で cargo の fmt、clippy、test を実行する
    fn run_rust(&self, dir: &str) -> Step {
        let cargo_in = |args: &[&str]| -> (Vec<String>, Vec<OsString>) {
            let mut display = vec!["cargo_in".to_string(), dir.to_string()];
            let mut argv: Vec<OsString> = self.cargo_pin.iter().map(OsString::from).collect();
            argv.push("cargo".into());
            for arg in args {
                if *arg == "<jobs>" {
                    display.extend(self.cargo_jobs.iter().cloned());
                    argv.extend(self.cargo_jobs.iter().map(OsString::from));
                } else {
                    display.push(arg.to_string());
                    argv.push(arg.into());
                }
            }
            (display, argv)
        };
        // 修正コマンドはコピーしてそのまま実行できるよう、空白や - で始まるパスも引用する
        let fix = format!("(cd -- {} && cargo fmt)", shell_quote(dir));
        let dir = Path::new(dir);
        for (stage, fix, args) in [
            ("format", fix.as_str(), &["fmt", "--check"][..]),
            (
                "lint",
                "",
                &["clippy", "<jobs>", "--all-targets", "--", "-D", "warnings"][..],
            ),
            ("test", "", &["test", "<jobs>"][..]),
        ] {
            let (display, argv) = cargo_in(args);
            self.stage("rust", stage, "cargo", fix, &display, || {
                spawn(&argv, Some(dir))
            })?;
        }
        Ok(())
    }

    /// 差分が空でなく、すべて .md か。削除と rename の前後も数える（--diff-filter=ACMR の changed_files では、.rs の
    /// 削除や .rs から .md への rename が見えない）。.md 以外は、.rs と Cargo.toml に限らず clippy.toml やテストが読む
    /// データも Rust の結果を変えうるので、すべて全体を実行する側に数える
    fn docs_only_diff(&self) -> Result<bool, i32> {
        let (Some(all), Some(rest)) = (
            self.diff_names(&["--no-renames"]),
            self.diff_names(&["--no-renames", "--", ":(exclude)*.md"]),
        ) else {
            println!("FAILED: rust docs-test: 差分（git diff）を読めません");
            return Err(1);
        };
        let blank = |b: &[u8]| b.iter().all(|c| *c == b'\n');
        Ok(!blank(&all) && blank(&rest))
    }

    fn run_rust_docs_tests(&self) -> Step {
        let mut argv = self.cargo_pin.clone();
        argv.extend(["bash".to_string(), RUST_DOCS_TESTS.to_string()]);
        self.command_stage("rust", "docs-test", "bash", "", &argv)
    }

    /// ステージ済みの .rs ごとに、上へたどって最も近い Cargo.toml のディレクトリ（crate）。crate に属さない .rs は stray
    fn rust_targets(&self) -> (Vec<String>, Vec<String>) {
        let mut crates = BTreeSet::new();
        let mut stray = BTreeSet::new();
        for file in self.changed_files(&["*.rs"]).unwrap_or_default() {
            let parent = |p: &Path| -> PathBuf {
                match p.parent() {
                    Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
                    _ => PathBuf::from("."),
                }
            };
            let mut dir = parent(&file);
            while dir != Path::new(".") && !dir.join("Cargo.toml").is_file() {
                dir = parent(&dir);
            }
            if dir.join("Cargo.toml").is_file() {
                crates.insert(dir.to_string_lossy().into_owned());
            } else {
                stray.insert(file.to_string_lossy().into_owned());
            }
        }
        (crates.into_iter().collect(), stray.into_iter().collect())
    }

    fn rust(&self) -> Result<bool, i32> {
        let manifest = is_file("Cargo.toml");
        if manifest && Path::new(RUST_DOCS_TESTS).exists() {
            if !is_executable(RUST_DOCS_TESTS) {
                println!("FAILED: rust docs-test: {RUST_DOCS_TESTS} に実行権限が無い（chmod +x）");
                return Err(1);
            }
            if self.docs_only_diff()? {
                for stage in ["format", "lint", "test"] {
                    println!(
                        "SKIP: rust {stage} (差分が .md だけなので {RUST_DOCS_TESTS} だけを実行する)"
                    );
                }
            } else {
                self.run_rust(".")?;
            }
            self.run_rust_docs_tests()?;
        } else if manifest {
            self.run_rust(".")?;
        } else if self.staged_has("*.rs") {
            let (crates, stray) = self.rust_targets();
            for dir in &crates {
                self.run_rust(dir)?;
            }
            if !stray.is_empty() {
                println!(
                    "SKIP: rust (Cargo.toml の無い場所の .rs: {})",
                    stray.join(" ")
                );
            }
        } else {
            return Ok(false);
        }
        Ok(true)
    }

    // --- Python ---

    fn python(&self) -> Result<bool, i32> {
        if !(["pyproject.toml", "setup.py", "requirements.txt"]
            .iter()
            .any(|f| is_file(f))
            || self.staged_has("*.py"))
        {
            return Ok(false);
        }
        let argv = |args: &[&str]| args.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        self.command_stage(
            "python",
            "format",
            "ruff",
            "ruff format .",
            &argv(&["ruff", "format", "--check", "."]),
        )?;
        self.command_stage(
            "python",
            "lint",
            "ruff",
            "ruff check --fix .",
            &argv(&["ruff", "check", "."]),
        )?;
        if Path::new(PYTHON_TESTS).exists() && !is_executable(PYTHON_TESTS) {
            println!("FAILED: python test: {PYTHON_TESTS} に実行権限が無い（chmod +x）");
            return Err(1);
        } else if Path::new(PYTHON_TESTS).exists() {
            // リポジトリ固有の実行方法（devShell を分けるなど）
            self.command_stage("python", "test", "bash", "", &argv(&["bash", PYTHON_TESTS]))?;
        } else if Path::new("tests").is_dir() || root_has_test_file() {
            self.command_stage("python", "test", "pytest", "", &argv(&["pytest"]))?;
        } else if let Some(files) = python_test_files().filter(|f| !f.is_empty()) {
            let mut display = vec!["python_subdir_tests".to_string()];
            display.extend(files.iter().cloned());
            self.stage("python", "test", "python3", "", &display, || {
                python_subdir_tests(&files)
            })?;
        } else {
            println!("SKIP: python test (テストが見つからない)");
        }
        Ok(true)
    }

    // --- Nix ---

    fn nix(&self) -> Result<bool, i32> {
        let flake = is_file("flake.nix");
        if !(flake || self.staged_has("*.nix")) {
            return Ok(false);
        }
        self.stage(
            "nix",
            "format",
            "nixfmt",
            "git ls-files -z '*.nix' | xargs -0 -r nixfmt",
            &["nix_fmt_check".to_string()],
            || {
                git_bytes(&["ls-files", "-z", "*.nix"])
                    .is_some_and(|out| xargs(&["nixfmt", "--check"], &nul_paths(&out)))
            },
        )?;
        self.command_stage(
            "nix",
            "lint",
            "statix",
            "statix fix",
            &["statix".to_string(), "check".to_string()],
        )?;
        if flake {
            let mut argv = vec!["nix".to_string(), "flake".to_string(), "check".to_string()];
            argv.extend(self.nix_limit.iter().cloned());
            self.command_stage("nix", "test", "nix", "", &argv)?;
        } else {
            println!("SKIP: nix test (flake.nix がない)");
        }
        Ok(true)
    }

    // --- Markdown ---

    fn markdown(&self) -> Result<bool, i32> {
        let mut detected = false;
        if self.staged_has("*.md") {
            detected = true;
            if self.merge {
                println!("SKIP: markdown format (--merge では自動修正しない)");
                self.stage(
                    "markdown",
                    "lint",
                    "markdownlint",
                    "",
                    &["md_lint".to_string()],
                    || {
                        self.changed_files(&["*.md"])
                            .is_some_and(|files| xargs(&["markdownlint", "--"], &files))
                    },
                )?;
            } else {
                // ステージした版を直してステージし直し、検査する（グローバルの pre-commit と同じ staged-checks。作業ツリーを
                // 書き換えないので、一部だけステージした .md のステージしていない編集をステージしない）
                self.command_stage(
                    "markdown",
                    "staged",
                    "staged-checks",
                    "",
                    &["staged-checks".to_string(), "markdown".to_string()],
                )?;
            }
        }
        if design_records_opted_in()? {
            detected = true;
            self.stage(
                "markdown",
                "design-records",
                "design-records",
                "",
                &["design_records_check".to_string()],
                || self.design_records_check(),
            )?;
        }
        Ok(detected)
    }

    /// ADR の形と ADR どうしの参照。変更したファイルを全部渡し、置き換えられた ADR への参照をそこから探させる（prek の
    /// 設定の types: [text] と同じ。テキストでないファイルは design-records が読み飛ばす）。--changed で、触っていない ADR の
    /// 推奨の節の warning を出さない。分けて呼ばないのは、ADR どうしの検査の指摘が重なるため
    fn design_records_check(&self) -> bool {
        let Some(files) = self.changed_files(&[]) else {
            return false;
        };
        let mut argv = os(&["design-records", "check", "--changed"]);
        argv.extend(files.into_iter().map(PathBuf::into_os_string));
        spawn(&argv, None)
    }

    // --- Shell ---

    fn shell(&self) -> Result<bool, i32> {
        if !self.staged_has("*.sh") {
            return Ok(false);
        }
        self.stage(
            "shell",
            "lint",
            "shellcheck",
            "",
            &["sh_lint".to_string()],
            || {
                self.changed_files(&["*.sh"])
                    .is_some_and(|files| xargs(&["shellcheck", "-S", "warning", "--"], &files))
            },
        )?;
        Ok(true)
    }
}

/// taskset で限る CPU（オンラインの最後のもの。`getconf _NPROCESSORS_ONLN` から 1 を引く）
fn last_cpu() -> usize {
    let online = Command::new("getconf")
        .arg("_NPROCESSORS_ONLN")
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<usize>()
                .ok()
        })
        .or_else(|| std::thread::available_parallelism().ok().map(usize::from))
        .unwrap_or(1);
    online.saturating_sub(1)
}

/// ルートに test_*.py か *_test.py があるか（sh の `compgen -G` と同じく、. で始まる名前は * に一致しない）
fn root_has_test_file() -> bool {
    std::fs::read_dir(".").is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            (name.starts_with("test_") && name.ends_with(".py"))
                || (!name.starts_with('.') && name.ends_with("_test.py"))
        })
    })
}

/// git が追跡している test_*.py / *_test.py（サブディレクトリを含む。無視したディレクトリや references/ は追跡されない
/// ので入らない）。git が失敗すれば None
fn python_test_files() -> Option<Vec<String>> {
    git_bytes(&[
        "ls-files",
        "--",
        ":(glob)**/test_*.py",
        ":(glob)**/*_test.py",
    ])
    .map(|out| {
        String::from_utf8_lossy(&out)
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect()
    })
}

/// 追跡しているテストを、ディレクトリごとに実行する。テストは隣の .py を import するので、そのディレクトリを起点に
/// する。ディレクトリやパターンでなく追跡しているファイルだけを渡すのは、子のディレクトリのテストを親と子で二重に
/// 実行せず、追跡していないファイルを拾わないため。pytest が python3 から使えれば pytest、無ければ標準の unittest
/// （pytest を足さない環境で、テストが走らないまま通るのを避ける）
fn python_subdir_tests(files: &[String]) -> bool {
    let pytest_ok = find_on_path("python3").is_some_and(|python| {
        Command::new(python)
            .args(["-m", "pytest", "--version"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    });
    // スラッシュの無いパス（ルート直下）は . とみなす
    let split = |f: &str| -> (String, String) {
        match f.rsplit_once('/') {
            Some((dir, name)) => (dir.to_string(), name.to_string()),
            None => (".".to_string(), f.to_string()),
        }
    };
    let dirs: BTreeSet<String> = files.iter().map(|f| split(f).0).collect();
    for dir in dirs {
        let names: Vec<String> = files
            .iter()
            .map(|f| split(f))
            .filter(|(d, _)| *d == dir)
            .map(|(_, name)| name)
            .collect();
        let mut cmd = vec!["python3".to_string(), "-m".to_string()];
        if pytest_ok {
            cmd.push("pytest".to_string());
            cmd.extend(names);
        } else {
            cmd.push("unittest".to_string());
            cmd.extend(
                names
                    .iter()
                    .map(|n| n.strip_suffix(".py").unwrap_or(n).to_string()),
            );
        }
        let line = format!("cd {} && {}", shell_quote(&dir), cmd.join(" "));
        println!("RUN: python test: {line}");
        let argv: Vec<OsString> = cmd.iter().map(OsString::from).collect();
        if !spawn(&argv, Some(Path::new(&dir))) {
            println!("FAILED: python test: {line}");
            return false;
        }
    }
    true
}

/// design-records は .pre-commit-config.yaml でオプトインしたリポジトリだけで実行する（docs/adr の有無で決めると、
/// 別の形の ADR を持つ他人のプロジェクトでも止まる）。.md の変更が無くても実行し、ADR を消すだけの変更でも、消した
/// ADR を指す参照が残っていないかを見る（prek の設定の always_run と同じ）。読めなければ止める
fn design_records_opted_in() -> Result<bool, i32> {
    let path = ".pre-commit-config.yaml";
    if !is_file(path) {
        return Ok(false);
    }
    let text = match std::fs::read(path) {
        Ok(text) => text,
        Err(e) => {
            println!("FAILED: markdown design-records: {path} を読めません（{e}）");
            return Err(1);
        }
    };
    let id = regex::bytes::Regex::new(
        r#"(?-u)^[[:space:]]*-?[[:space:]]*id:[[:space:]]*["']?design-records["']?[[:space:]]*(#.*)?$"#,
    )
    .expect("正しい正規表現");
    Ok(text.split(|b| *b == b'\n').any(|line| id.is_match(line)))
}
