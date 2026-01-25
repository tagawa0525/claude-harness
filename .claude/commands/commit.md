---
allowed-tools:
  - Bash(git *)
  - Bash(cargo *)
  - Bash(ruff *)
  - Bash(pytest*)
  - Bash(nixpkgs-fmt *)
  - Bash(statix *)
  - Bash(nix flake check*)
  - Bash(command -v *)
  - Bash(test -f *)
  - Bash(find *)
---

# Commit Command with Language Quality Checks

このコマンドは、言語ごとの品質チェック（フォーマット、リント、テスト）を実行してから git commit を作成します。

## Context

まず、以下の情報を並列で取得してください:

```bash
# Git status (never use -uall flag)
git status

# Git diff (staged and unstaged)
git diff HEAD

# Current branch
git branch --show-current

# Recent commits for commit message style
git log -5 --oneline
```

## Language Detection

staged files の拡張子とプロジェクトファイルから言語を検出:

```bash
# Get staged files
git diff --cached --name-only
```

**検出ルール**:
- Rust: `Cargo.toml` が存在する、または `.rs` ファイルがstaged
- Python: `pyproject.toml`, `setup.py`, `requirements.txt` のいずれかが存在する、または `.py` ファイルがstaged
- Nix: `flake.nix` が存在する、または `.nix` ファイルがstaged

## Quality Checks

検出された言語ごとに、以下のチェックを**順番に**実行してください。いずれかが失敗した場合は、即座に中断してエラーメッセージを表示し、commit を作成しないでください。

### Rust Checks

**実行条件**: Rustプロジェクトが検出された場合

```bash
# 1. Format check
echo "🔍 Checking Rust format..."
cargo fmt --check

# 2. Lint check
echo "🔍 Running Rust linter..."
cargo clippy --all-targets -- -D warnings

# 3. Test
echo "🧪 Running Rust tests..."
cargo test
```

**失敗時の対応**:
- Format失敗: `cargo fmt` を実行するよう提案
- Clippy失敗: エラー箇所を表示し、手動修正を要求
- Test失敗: テスト出力を表示し、手動修正を要求

### Python Checks

**実行条件**: Pythonプロジェクトが検出された場合

```bash
# 1. Format check
echo "🔍 Checking Python format..."
ruff format --check .

# 2. Lint check
echo "🔍 Running Python linter..."
ruff check .

# 3. Test (if tests exist)
if [ -d "tests" ] || [ -f "pytest.ini" ] || [ -f "pyproject.toml" ]; then
  echo "🧪 Running Python tests..."
  pytest
fi
```

**失敗時の対応**:
- Format失敗: `ruff format .` を実行するよう提案
- Lint失敗: `ruff check --fix .` で自動修正可能か確認し、提案
- Test失敗: テスト出力を表示し、手動修正を要求

### Nix Checks

**実行条件**: Nixプロジェクトが検出された場合

```bash
# 1. Format check (only for .nix files)
echo "🔍 Checking Nix format..."
nixpkgs-fmt --check *.nix

# 2. Static analysis
echo "🔍 Running Nix static analysis..."
statix check

# 3. Flake check (if flake.nix exists)
if [ -f "flake.nix" ]; then
  echo "🔍 Checking Nix flake..."
  nix flake check
fi
```

**失敗時の対応**:
- Format失敗: `nixpkgs-fmt *.nix` を実行するよう提案
- Statix失敗: エラー箇所を表示し、手動修正を要求
- Flake check失敗: エラー詳細を表示し、手動修正を要求

## Tool Availability Check

チェック実行前に、必要なツールがインストールされているか確認してください:

```bash
# Example for Rust
if ! command -v cargo >/dev/null 2>&1; then
  echo "⚠️  cargo not found. Skipping Rust checks."
fi
```

ツールが見つからない場合は、そのチェックをスキップして警告を表示してください。

## Error Handling Template

チェックが失敗した場合、以下の形式でエラーを表示してください:

```
❌ [言語] [チェック種類] check failed

Command: [実行したコマンド]

Error output:
[エラー出力]

💡 Fix suggestion:
[修正方法の提案]

⚠️  Commit aborted. Please fix the issues above and try again.
```

## Success: Create Commit

**全てのチェックが成功した場合のみ**、以下の手順で commit を作成してください:

1. 変更内容を分析し、conventional commits 形式でコミットメッセージを作成
2. staged files を確認
3. commit を作成:

```bash
git commit -m "$(cat <<'EOF'
[type]: [subject]

[optional body]

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>
EOF
)"
```

4. commit 後に `git status` を実行して成功を確認

## Commit Message Guidelines

- **Type**: feat, fix, docs, style, refactor, test, chore のいずれか
- **Subject**: 50文字以内、命令形、先頭大文字、末尾にピリオドなし
- **Body** (オプション): 変更の理由や詳細を記載

## Example Workflow

```
1. git status を確認
2. staged files から言語を検出 → Rust と Nix を検出
3. Rust checks を実行:
   - cargo fmt --check ✅
   - cargo clippy ✅
   - cargo test ✅
4. Nix checks を実行:
   - nixpkgs-fmt --check ✅
   - statix check ✅
   - nix flake check ✅
5. 全てのチェック成功 → commit 作成
6. "feat: add language quality checks to commit command"
```

## Important Notes

- **並列実行しない**: チェックは順番に実行し、失敗時は即座に中断
- **staged files のみ**: プロジェクト全体ではなく、staged files に関連するチェックのみ
- **明確なフィードバック**: どのチェックが失敗したか、どう修正するかを明示
- **commit 中断**: チェック失敗時は絶対に commit を作成しない
