#!/usr/bin/env python3
"""mdbook preprocessor stripping OKF frontmatter, which mdbook would otherwise
render as a stray horizontal rule and heading.

Self-check: python3 okf-frontmatter.py selftest
"""

import json
import re
import sys

FRONTMATTER = re.compile(r"\A---\r?\n.*?\r?\n---[ \t]*\r?\n", re.DOTALL)


def strip(items):
    for item in items:
        # Separators arrive as the bare string "Separator", not an object.
        if not isinstance(item, dict) or "Chapter" not in item:
            continue
        chapter = item["Chapter"]
        chapter["content"] = FRONTMATTER.sub("", chapter["content"], count=1)
        strip(chapter["sub_items"])


def selftest():
    def chapter(content, subs=()):
        return {"Chapter": {"content": content, "sub_items": list(subs)}}

    nested = chapter("---\ntype: Metric\n---\nnested")
    book = {
        "items": [
            chapter("---\ntype: Concept\nstatus: active\n---\n# Title\n", [nested]),
            chapter("# No frontmatter\n\n---\n\nrule above stays\n"),
            chapter("---\ntype: A\n---\nfirst only\n---\ntype: B\n---\n"),
            "Separator",
            {"PartTitle": "Part"},
        ]
    }
    strip(book["items"])
    got = [s["Chapter"]["content"] for s in book["items"] if "Chapter" in s]
    assert got[0] == "# Title\n", got[0]
    assert got[1] == "# No frontmatter\n\n---\n\nrule above stays\n", got[1]
    assert got[2] == "first only\n---\ntype: B\n---\n", got[2]
    assert nested["Chapter"]["content"] == "nested", nested
    print("ok")


if __name__ == "__main__":
    if sys.argv[1:2] == ["selftest"]:
        selftest()
    elif sys.argv[1:2] == ["supports"]:
        sys.exit(0)
    else:
        _context, book = json.load(sys.stdin)
        strip(book["items"])
        json.dump(book, sys.stdout)
