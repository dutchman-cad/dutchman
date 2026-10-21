#!/usr/bin/env python3
"""Writes the README a release archive carries: the repository README with every relative
link and image made absolute, pinned to the release's tag, since the archive holds none of
the files they point to.

    scripts/archive-readme.py <tag> <README.md> <output>
"""

import re
import sys

REPO = "https://github.com/dutchman-cad/dutchman"
RAW = "https://raw.githubusercontent.com/dutchman-cad/dutchman"


def absolute(target: str, tag: str, image: bool) -> str:
    if re.match(r"^[a-z][a-z0-9+.-]*:|^#|^/", target):
        return target
    if image:
        return f"{RAW}/{tag}/{target}"
    return f"{REPO}/blob/{tag}/{target}"


def rewrite(text: str, tag: str) -> str:
    # Markdown images, then links.
    text = re.sub(
        r"(!\[[^\]]*\]\()([^)\s]+)",
        lambda m: m.group(1) + absolute(m.group(2), tag, image=True),
        text,
    )
    text = re.sub(
        r"((?<!!)\[[^\]]*\]\()([^)\s]+)",
        lambda m: m.group(1) + absolute(m.group(2), tag, image=False),
        text,
    )
    # HTML src and srcset attributes.
    return re.sub(
        r'\b(src|srcset)="([^"]+)"',
        lambda m: f'{m.group(1)}="{absolute(m.group(2), tag, image=True)}"',
        text,
    )


def main() -> None:
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    tag, source, output = sys.argv[1:]
    with open(source, encoding="utf-8") as f:
        text = f.read()
    with open(output, "w", encoding="utf-8") as f:
        f.write(rewrite(text, tag))


if __name__ == "__main__":
    main()
