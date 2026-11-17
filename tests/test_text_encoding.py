"""Build scripts and tests read and write text as UTF-8 whatever the machine's locale."""

import ast
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[1]


def _constant(node):
    return node.value if isinstance(node, ast.Constant) else None


def text_io_without_utf8(source: str) -> list[int]:
    """Lines of text-mode open(), Path.open(), read_text() or write_text() calls
    that do not pass encoding="utf-8"."""
    lines = []
    for node in ast.walk(ast.parse(source)):
        if not isinstance(node, ast.Call):
            continue
        func = node.func
        keywords = {keyword.arg: keyword.value for keyword in node.keywords}
        if isinstance(func, ast.Attribute) and func.attr in ("read_text", "write_text"):
            mode = "r"
        elif isinstance(func, ast.Name) and func.id == "open":
            mode = keywords["mode"] if "mode" in keywords else node.args[1] if len(node.args) > 1 else "r"
        elif isinstance(func, ast.Attribute) and func.attr == "open":
            # Path.open takes the mode first. Other .open() calls (an opener's
            # request, Pillow's file object) take something else and are skipped.
            if "mode" in keywords:
                mode = keywords["mode"]
            elif not node.args:
                mode = "r"
            elif isinstance(_constant(node.args[0]), str):
                mode = node.args[0]
            else:
                continue
        else:
            continue
        mode = mode if isinstance(mode, str) else _constant(mode)
        if isinstance(mode, str) and "b" in mode:
            continue
        if str(_constant(keywords.get("encoding"))).lower() not in ("utf-8", "utf8"):
            lines.append(node.lineno)
    return sorted(lines)


class TextEncodingTests(unittest.TestCase):
    def test_scripts_and_tests_use_utf8_for_every_text_file(self):
        paths = sorted(ROOT.glob("scripts/*.py")) + sorted(ROOT.glob("tests/*.py"))
        missing = [f"{path.relative_to(ROOT)}:{line}" for path in paths
                   for line in text_io_without_utf8(path.read_text(encoding="utf-8"))]
        self.assertEqual(missing, [], 'Pass encoding="utf-8" to text file I/O')

    def test_checker_flags_only_text_io_without_utf8(self):
        source = "\n".join((
            "open(p)",
            'open(p, "w")',
            'open(p, "rb")',
            'open(p, "a", encoding="utf-8")',
            "p.read_text()",
            "p.write_text(text)",
            'p.write_text(text, encoding="latin-1")',
            'p.read_text(encoding="utf-8")',
            "p.open()",
            'p.open("a")',
            'p.open(mode="wb")',
            'p.open("r", encoding="utf8")',
            "opener.open(request, timeout=30)",
            "Image.open(io.BytesIO(data))",
        ))
        self.assertEqual(text_io_without_utf8(source), [1, 2, 5, 6, 7, 9, 10])


if __name__ == "__main__":
    unittest.main()
