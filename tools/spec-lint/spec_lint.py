#!/usr/bin/env python3
"""Check the Joker specification for broken remissions and leftover Portuguese.

Usage:

    python3 tools/spec-lint/spec_lint.py [PATH ...] [options]

PATH is a file or a directory; directories are walked for ``.md``, ``.wit``,
``.cddl``, ``.yaml``, ``.h``, ``.c`` and ``.toml`` files. Without a PATH the
tree is checked: ``docs/``, ``spec/``, ``wit/``, ``README.md`` and
``AGENTS.md`` under the repository root.

Two checks run, and each can be selected alone:

* links (``--links-only``): in Markdown files, outside code blocks and inline
  code, every relative link ``[text](target)``, every reference definition
  ``[id]: target`` and every mention of a ``.md`` path must resolve to an
  existing file, and every ``#anchor`` must match a heading (GitHub slug
  rules) or an explicit ``<a id>`` in the target. Mentions of the form
  ``NN-slug:line`` must name an existing document stem. A bare file name is
  only checked when it starts with a digit, so planned files such as
  ``SECURITY.md`` are not reported. Lines between
  ``<!-- spec-lint: links-off -->`` and ``<!-- spec-lint: links-on -->`` are
  not checked. Broken links make the exit status 1.
* Portuguese (``--portuguese-only``): every file gets a score, the percentage
  of its prose words that are Portuguese markers (function words that do not
  exist in English, and words with Portuguese-only diacritics). Code blocks,
  inline code, URLs and link targets are ignored in Markdown. Lines with
  several markers are listed with ``--verbose``. A file that contains the
  directive ``<!-- spec-lint: allow-portuguese -->`` is skipped. The scores
  are a report; with ``--max-score N`` any file above N makes the exit status
  1, which is how a translated file is checked (``--max-score 0.5``).

``--json`` prints one JSON document instead of text. The tool uses only the
Python 3 standard library.
"""

import argparse
import json
import os
import re
import sys
import unicodedata
from pathlib import Path

ALLOW_DIRECTIVE = "<!-- spec-lint: allow-portuguese -->"
LINKS_OFF = "<!-- spec-lint: links-off -->"
LINKS_ON = "<!-- spec-lint: links-on -->"
TEXT_SUFFIXES = {".md", ".wit", ".cddl", ".yaml", ".yml", ".h", ".c", ".toml"}
DEFAULT_TARGETS = ["docs", "spec", "wit", "README.md", "AGENTS.md"]

PT_FUNCTION_WORDS = frozenset(
    """
    não nao que de da dos das em na nos nas num numa nuns numas um uma
    uns umas é são sao para pelo pela pelos pelas ao aos à às ou mas como
    mais quando também tambem só já ser está estão esta este estes estas
    esse essa esses essas isso isto aquilo seu sua seus suas onde porque
    porquê sem até cada depois antes ainda sempre nunca qual quais quem há
    mesmo mesma mesmos mesmas outro outra outros outras tem têm fica ficam
    pode podem deve devem faz fazem vai vão foi foram ele ela eles elas
    nenhum nenhuma toda todos todas muito muita pouco entre sobre
    contra desde durante enquanto então senão aqui ali lá dele dela deles
    delas neste nesta nesse nessa disso disto deste desta desse dessa
    qualquer quaisquer tu teu tua teus tuas vais tens és estás seja sejam
    """.split()
)

PT_DIACRITICS = re.compile(r"[ãõçâêôàÃÕÇÂÊÔÀ]")
WORD = re.compile(r"[^\W\d_]+(?:['’-][^\W\d_]+)*", re.UNICODE)
FENCE = re.compile(r"^\s*(```|~~~)")
INLINE_CODE = re.compile(r"(`+)(?:(?!\1).)+?\1")
URL = re.compile(r"\b[a-z][a-z0-9+.-]*://\S+", re.IGNORECASE)
INLINE_LINK = re.compile(r"!?\[((?:[^\[\]]|\[[^\]]*\])*)\]\(\s*<?([^)\s>]*)>?(?:\s+\"[^\"]*\")?\s*\)")
REF_DEF = re.compile(r"^\s{0,3}\[([^\]]+)\]:\s+<?(\S+?)>?(?:\s+.*)?$")
HEADING = re.compile(r"^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$")
EXPLICIT_ANCHOR = re.compile(r"<a\s+(?:id|name)\s*=\s*\"([^\"]+)\"", re.IGNORECASE)
HTML_COMMENT = re.compile(r"<!--.*?-->", re.DOTALL)
MD_PATH = re.compile(r"(?<![\w./-])((?:\.\.?/)*(?:[\w.-]+/)*[\w.-]+\.md)(#[\w-]+)?(?![\w/-])", re.UNICODE)
STEM_LINE = re.compile(r"(?<![\w/-])((?:[a-z]+/)?\d{2,4}-[a-z0-9]+(?:-[a-z0-9]+)*):\d+")
SCHEME = re.compile(r"^[a-z][a-z0-9+.-]*:", re.IGNORECASE)


def find_root(start):
    """Return the repository root: the nearest ancestor holding AGENTS.md and docs/."""
    here = Path(start).resolve()
    for candidate in [here, *here.parents]:
        if (candidate / "AGENTS.md").is_file() and (candidate / "docs").is_dir():
            return candidate
    return Path.cwd().resolve()


def collect_files(targets, root):
    """Expand files and directories into a sorted list of checkable files."""
    files = set()
    for target in targets:
        path = Path(target)
        if not path.is_absolute():
            path = (Path.cwd() / path) if path.exists() else (root / path)
        if path.is_file():
            files.add(path.resolve())
        elif path.is_dir():
            for dirpath, dirnames, filenames in os.walk(path):
                dirnames[:] = sorted(d for d in dirnames if not d.startswith(".") and d != "target")
                for name in filenames:
                    candidate = Path(dirpath) / name
                    if candidate.suffix in TEXT_SUFFIXES:
                        files.add(candidate.resolve())
        else:
            print(f"spec-lint: no such file or directory: {target}", file=sys.stderr)
    return sorted(files)


def prose_lines(text):
    """Yield (line number, line) for Markdown lines outside fenced code blocks."""
    in_fence = None
    for number, line in enumerate(text.split("\n"), start=1):
        match = FENCE.match(line)
        if match:
            marker = match.group(1)
            if in_fence is None:
                in_fence = marker
                continue
            if marker == in_fence:
                in_fence = None
                continue
        if in_fence is None:
            yield number, line


def strip_inline(line):
    """Remove inline code spans and HTML comments from a Markdown line."""
    return INLINE_CODE.sub(" ", HTML_COMMENT.sub(" ", line))


def github_slug(heading):
    """Return the anchor GitHub generates for a heading text."""
    text = INLINE_LINK.sub(lambda m: m.group(1), heading)
    text = re.sub(r"<[^>]+>", "", text).strip().lower()
    kept = []
    for char in text:
        if char.isalnum() or char in "-_" or unicodedata.category(char).startswith("M"):
            kept.append(char)
        elif char == " ":
            kept.append("-")
    return "".join(kept)


class AnchorCache:
    """Compute and remember the anchors defined by each Markdown file."""

    def __init__(self):
        """Create an empty cache."""
        self._anchors = {}

    def get(self, path):
        """Return the set of anchors of a Markdown file."""
        path = path.resolve()
        if path not in self._anchors:
            anchors = set()
            counts = {}
            text = path.read_text(encoding="utf-8", errors="replace")
            for _, line in prose_lines(text):
                for match in EXPLICIT_ANCHOR.finditer(line):
                    anchors.add(match.group(1))
                heading = HEADING.match(line)
                if heading:
                    slug = github_slug(heading.group(2))
                    seen = counts.get(slug, 0)
                    counts[slug] = seen + 1
                    anchors.add(slug if seen == 0 else f"{slug}-{seen}")
            self._anchors[path] = anchors
        return self._anchors[path]


def doc_stems(root):
    """Return every Markdown stem under docs/, bare and with its directory prefix."""
    stems = set()
    docs = root / "docs"
    for path in docs.rglob("*.md"):
        stems.add(path.stem)
        rel = path.relative_to(docs).with_suffix("")
        stems.add(rel.as_posix())
    return stems


def resolve_mention(target, source, root):
    """Resolve a bare .md path mention against the source directory, docs/ and the root."""
    bases = [source.parent, root / "docs", root]
    if source.is_relative_to(root / "docs"):
        bases.insert(1, root / "docs" / source.relative_to(root / "docs").parts[0])
    for base in bases:
        candidate = (base / target).resolve()
        if candidate.is_file():
            return candidate
    return None


def check_links(path, root, anchors, stems):
    """Return the broken links and remissions of one Markdown file."""
    problems = []
    text = path.read_text(encoding="utf-8", errors="replace")

    def check_target(number, target, kind):
        """Check one explicit link target and record a problem if it is broken."""
        if not target or SCHEME.match(target):
            return
        file_part, _, fragment = target.partition("#")
        destination = path if not file_part else (path.parent / file_part).resolve()
        if not destination.exists():
            problems.append({"line": number, "kind": kind, "target": target, "problem": "missing file"})
            return
        if fragment and destination.suffix == ".md" and fragment not in anchors.get(destination):
            problems.append({"line": number, "kind": kind, "target": target, "problem": "missing anchor"})

    enabled = True
    for number, raw in prose_lines(text):
        if LINKS_OFF in raw:
            enabled = False
        if LINKS_ON in raw:
            enabled = True
        if not enabled:
            continue
        line = strip_inline(raw)
        for match in INLINE_LINK.finditer(line):
            check_target(number, match.group(2), "link")
        definition = REF_DEF.match(line)
        if definition:
            check_target(number, definition.group(2), "reference")
        mention_text = URL.sub(" ", INLINE_LINK.sub(lambda m: m.group(1), HTML_COMMENT.sub(" ", raw)))
        for match in MD_PATH.finditer(mention_text):
            target, fragment = match.group(1), (match.group(2) or "")[1:]
            if "<" in target or "*" in target:
                continue
            if "/" not in target and not target[:1].isdigit():
                continue
            destination = resolve_mention(target, path, root)
            if destination is None:
                problems.append({"line": number, "kind": "mention", "target": target, "problem": "missing file"})
            elif fragment and fragment not in anchors.get(destination):
                problems.append({"line": number, "kind": "mention", "target": f"{target}#{fragment}", "problem": "missing anchor"})
        for match in STEM_LINE.finditer(mention_text):
            if match.group(1) not in stems:
                problems.append({"line": number, "kind": "stem", "target": match.group(1), "problem": "unknown document"})
    return problems


def portuguese_score(path):
    """Return the Portuguese score of a file and the lines that look Portuguese."""
    text = path.read_text(encoding="utf-8", errors="replace")
    if ALLOW_DIRECTIVE in text:
        return None
    if path.suffix == ".md":
        lines = ((n, strip_inline(INLINE_LINK.sub(lambda m: m.group(1), l))) for n, l in prose_lines(text))
    else:
        lines = enumerate(text.split("\n"), start=1)
    total = 0
    markers = 0
    flagged = []
    for number, line in lines:
        words = WORD.findall(URL.sub(" ", line))
        hits = sum(1 for w in words if w.lower() in PT_FUNCTION_WORDS or PT_DIACRITICS.search(w))
        total += len(words)
        markers += hits
        if hits >= 3 or (hits >= 1 and hits * 4 >= len(words) and len(words) >= 2):
            flagged.append(number)
    score = round(100.0 * markers / total, 2) if total else 0.0
    return {"score": score, "words": total, "markers": markers, "flagged_lines": flagged}


def main(argv=None):
    """Run the checks and return the process exit status."""
    parser = argparse.ArgumentParser(description="Check the Joker spec for broken links and leftover Portuguese.")
    parser.add_argument("paths", nargs="*", help="files or directories (default: the spec tree)")
    selector = parser.add_mutually_exclusive_group()
    selector.add_argument("--links-only", action="store_true", help="only check links and remissions")
    selector.add_argument("--portuguese-only", action="store_true", help="only score leftover Portuguese")
    parser.add_argument("--max-score", type=float, default=None, help="fail when a file's Portuguese score exceeds this percentage")
    parser.add_argument("--json", action="store_true", help="print a JSON report")
    parser.add_argument("--verbose", action="store_true", help="list the line numbers that look Portuguese")
    args = parser.parse_args(argv)

    root = find_root(Path.cwd())
    files = collect_files(args.paths or DEFAULT_TARGETS, root)
    anchors = AnchorCache()
    stems = doc_stems(root)
    report = {"root": str(root), "files": []}
    broken = 0
    over = 0
    for path in files:
        try:
            shown = path.relative_to(root).as_posix()
        except ValueError:
            shown = str(path)
        entry = {"file": shown}
        if not args.portuguese_only and path.suffix == ".md":
            entry["links"] = check_links(path, root, anchors, stems)
            broken += len(entry["links"])
        if not args.links_only:
            result = portuguese_score(path)
            entry["portuguese"] = result
            if result and args.max_score is not None and result["score"] > args.max_score:
                over += 1
        report["files"].append(entry)
    report["broken_links"] = broken
    report["files_over_max_score"] = over

    if args.json:
        print(json.dumps(report, indent=2, ensure_ascii=False))
    else:
        for entry in report["files"]:
            for problem in entry.get("links", []):
                print(f"{entry['file']}:{problem['line']}: {problem['kind']} {problem['target']}: {problem['problem']}")
        if not args.links_only:
            scored = [e for e in report["files"] if e.get("portuguese")]
            scored.sort(key=lambda e: e["portuguese"]["score"], reverse=True)
            for entry in scored:
                result = entry["portuguese"]
                mark = " over" if args.max_score is not None and result["score"] > args.max_score else ""
                line = f"{result['score']:6.2f}%  {len(result['flagged_lines']):5d} lines  {entry['file']}{mark}"
                if args.verbose and result["flagged_lines"]:
                    line += "  [" + ",".join(map(str, result["flagged_lines"][:50])) + "]"
                print(line)
            words = sum(e["portuguese"]["words"] for e in scored)
            markers = sum(e["portuguese"]["markers"] for e in scored)
            overall = 100.0 * markers / words if words else 0.0
            print(f"portuguese: {len(scored)} files scored, overall {overall:.2f}%")
        if not args.portuguese_only:
            print(f"links: {broken} broken")
    return 1 if broken or over else 0


if __name__ == "__main__":
    sys.exit(main())
