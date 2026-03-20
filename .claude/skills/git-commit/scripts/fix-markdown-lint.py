#!/usr/bin/env python3
"""Fix markdown lint issues that markdownlint --fix cannot handle.

Usage:
    python3 fix-markdown-lint.py <file>...

Fixes:
    MD040  Fenced code blocks should have a language specified (via heuristic)
    MD060  Table column alignment (CJK full-width aware)
"""

import re
import sys
import unicodedata
from pathlib import Path


def display_width(s: str) -> int:
    """Calculate display width accounting for CJK wide characters.

    East Asian Width categories treated as width 2:
      F (Fullwidth), W (Wide), A (Ambiguous)
    Ambiguous characters (e.g. →, ←, ★, ●) display as width 2
    in CJK locale terminals, which is our target environment.
    """
    w = 0
    for c in s:
        eaw = unicodedata.east_asian_width(c)
        if eaw in ("F", "W", "A"):
            w += 2
        else:
            w += 1
    return w


def is_table_row(line: str) -> bool:
    stripped = line.strip()
    return stripped.startswith("|") and stripped.endswith("|") and "|" in stripped[1:-1]


def is_separator_row(line: str) -> bool:
    if not is_table_row(line):
        return False
    cells = [c.strip() for c in line.strip().strip("|").split("|")]
    pattern = re.compile(r"^:?-+:?$")
    non_empty_cells = [c for c in cells if c]
    dash_cells = [c for c in non_empty_cells if pattern.match(c)]
    return bool(dash_cells) and len(dash_cells) == len(non_empty_cells)


def format_table(table_lines: list[str]) -> list[str]:
    """Format a markdown table with aligned columns (CJK-aware)."""
    rows = []
    for line in table_lines:
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        rows.append(cells)

    if len(rows) < 2:
        return table_lines

    ncols = max(len(row) for row in rows)

    # Detect alignment from separator row
    alignments = []
    for cell in rows[1]:
        stripped = cell.strip()
        if stripped.startswith(":") and stripped.endswith(":"):
            alignments.append("center")
        elif stripped.endswith(":"):
            alignments.append("right")
        else:
            alignments.append("left")
    while len(alignments) < ncols:
        alignments.append("left")

    # Calculate column widths (skip separator row)
    col_widths = [0] * ncols
    for ri, row in enumerate(rows):
        if ri == 1:
            continue
        for i, cell in enumerate(row):
            if i < ncols:
                w = display_width(cell)
                if w > col_widths[i]:
                    col_widths[i] = w

    # Build formatted rows
    result = []
    for ri, row in enumerate(rows):
        parts = []
        for i in range(ncols):
            cell = row[i] if i < len(row) else ""
            cw = col_widths[i]

            if ri == 1:  # separator
                if alignments[i] == "center":
                    parts.append(" :" + "-" * max(1, cw - 2) + ": ")
                elif alignments[i] == "right":
                    parts.append(" " + "-" * max(1, cw - 1) + ": ")
                else:
                    parts.append(" " + "-" * max(1, cw) + " ")
                continue

            dw = display_width(cell)
            padding = cw - dw

            if alignments[i] == "right":
                parts.append(" " * (padding + 1) + cell + " ")
            elif alignments[i] == "center":
                lpad = padding // 2
                rpad = padding - lpad
                parts.append(" " * (lpad + 1) + cell + " " * (rpad + 1))
            else:
                parts.append(" " + cell + " " * (padding + 1))

        result.append("|" + "|".join(parts) + "|")

    return result


def guess_language(content_lines: list[str]) -> str:
    """Guess the language of a fenced code block from its content.

    Uses conservative patterns to avoid false positives.
    Order matters: more specific languages are checked first.
    """
    sample = "\n".join(content_lines)

    # Nix
    if re.search(
        r"(mkDerivation|buildInputs|pkgs\.|lib\.|fetchFromGitHub|stdenv"
        r"|mkOption|mkEnableOption|mkIf|with pkgs|inherit \(|\.nix\b"
        r"|home\.packages|programs\.\w+\.enable|nixpkgs\.overlays)",
        sample,
    ):
        return "nix"

    # Rust
    if re.search(
        r"(cargo |fn \w+\(|let \w+\s*[=:]|use \w+::\w+|pub |impl \w|struct \w|enum \w|trait \w"
        r"|#\[(derive|cfg|test|allow|deny|warn|must_use|inline)]"
        r"|\.unwrap\(\)|\.expect\(|-> \w+|&self|&mut )",
        sample,
    ):
        return "rust"

    # Python
    if re.search(
        r"(?m)(^def \w+\(|^class \w+|^import \w+|^from \w+ import"
        r"|if __name__|\.append\(|\.items\(\)|self\.\w+|print\(|raise \w+)",
        sample,
    ):
        return "python"

    # TypeScript (check before JavaScript)
    if re.search(
        r"(:\s*(string|number|boolean|void)\b|interface \w+|type \w+ =|<\w+>|as \w+"
        r"|import .+ from ['\"]|export (default |const |function |interface ))",
        sample,
    ):
        return "typescript"

    # JavaScript
    if re.search(
        r"(?m)(^const \w+|^let \w+|^var \w+|=>\s*\{|require\(['\"]"
        r"|module\.exports|console\.(log|error)|document\.|window\.)",
        sample,
    ):
        return "javascript"

    # JSON
    if re.search(r'^\s*\{[\s\S]*"[^"]+"\s*:', sample) and sample.rstrip().endswith("}"):
        return "json"

    # YAML
    if re.search(r"(?m)^[\w-]+:\s+\S", sample) and not re.search(r"[{};]", sample):
        return "yaml"

    # TOML
    if re.search(r"(?m)(^\[[\w.-]+\]$|^\w+\s*=\s*[\"'\[\{])", sample):
        return "toml"

    # HTML
    if re.search(r"(<html|<div|<span|<p>|<a |<img |<!DOCTYPE)", sample, re.IGNORECASE):
        return "html"

    # CSS
    if re.search(
        r"(?m)(^\s*\.\w+\s*\{|^\s*#\w+\s*\{|color:|font-size:|margin:|padding:)", sample
    ):
        return "css"

    # SQL
    if re.search(
        r"(?i)(SELECT .+ FROM|INSERT INTO|CREATE TABLE|ALTER TABLE|DROP TABLE|UPDATE .+ SET)",
        sample,
    ):
        return "sql"

    # Dockerfile
    if re.search(
        r"(?m)^(FROM |RUN |COPY |CMD |ENTRYPOINT |WORKDIR |EXPOSE |ENV )", sample
    ):
        return "dockerfile"

    # Shell / Bash (broad - check late)
    if re.search(
        r"(?m)(^(git |gh |npm |pip |mkdir |cd |ls |rm |cp |mv |curl |wget |chmod |chown )"
        r"|^\$ |^#!.*/bin/(ba)?sh|if \[|then$|fi$|\|\||&&)",
        sample,
    ):
        return "bash"

    return "text"


def fix_markdown(content: str) -> str:
    lines = content.split("\n")

    # Pass 1: Format tables (MD060 - CJK aware), skip fenced code blocks
    output: list[str] = []
    i = 0
    in_code = False
    while i < len(lines):
        line = lines[i]

        if line.strip().startswith("```"):
            in_code = not in_code

        if (
            not in_code
            and is_table_row(line)
            and i + 1 < len(lines)
            and is_separator_row(lines[i + 1])
        ):
            table = []
            j = i
            while j < len(lines) and is_table_row(lines[j]):
                table.append(lines[j])
                j += 1
            output.extend(format_table(table))
            i = j
            continue

        output.append(line)
        i += 1

    # Pass 2: Fix fenced code block languages (MD040)
    lines = output
    output = []
    in_code = False
    for i, line in enumerate(lines):
        if line.strip().startswith("```"):
            if not in_code:
                if line.strip() == "```":
                    content_lines = []
                    for j in range(i + 1, len(lines)):
                        if lines[j].strip().startswith("```"):
                            break
                        content_lines.append(lines[j])
                    lang = guess_language(content_lines)
                    output.append(f"```{lang}")
                else:
                    output.append(line)
                in_code = True
            else:
                output.append(line)
                in_code = False
        else:
            output.append(line)

    return "\n".join(output)


def main():
    files = [a for a in sys.argv[1:] if not a.startswith("--")]

    if not files:
        print(__doc__)
        sys.exit(1)

    for filepath in files:
        path = Path(filepath)
        if not path.exists():
            continue

        content = path.read_text(encoding="utf-8")
        fixed = fix_markdown(content)

        if content != fixed:
            path.write_text(fixed, encoding="utf-8", newline="\n")
            print(f"Fixed: {filepath}")
        else:
            print(f"OK: {filepath}")


if __name__ == "__main__":
    main()
