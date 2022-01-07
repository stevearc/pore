#!/usr/bin/env python
import json
import os
import os.path
import re
import subprocess
from typing import List

HERE = os.path.dirname(__file__)
ROOT = os.path.abspath(os.path.join(HERE, os.path.pardir))
README = os.path.join(ROOT, "README.md")


def replace_section(file: str, start_pat: str, end_pat: str, lines: List[str]) -> None:
    prefix_lines: List[str] = []
    postfix_lines: List[str] = []
    file_lines = prefix_lines
    found_section = False
    with open(file, "r", encoding="utf-8") as ifile:
        inside_section = False
        for line in ifile:
            if inside_section:
                if re.match(end_pat, line):
                    inside_section = False
                    file_lines = postfix_lines
                    file_lines.append(line)
            else:
                if re.match(start_pat, line):
                    inside_section = True
                    found_section = True
                file_lines.append(line)

    if inside_section or not found_section:
        raise Exception(f"could not find file section {start_pat}")

    all_lines = prefix_lines + lines + postfix_lines
    with open(file, "w", encoding="utf-8") as ofile:
        ofile.write("".join(all_lines))


def read_section(filename: str, start_pat: str, end_pat: str) -> List[str]:
    lines = []
    with open(filename, "r", encoding="utf-8") as ifile:
        inside_section = False
        for line in ifile:
            if inside_section:
                if re.match(end_pat, line):
                    break
                lines.append(line)
            elif re.match(start_pat, line):
                inside_section = True
    return lines


def main() -> None:
    """Update the README"""
    lines = subprocess.getoutput("cargo run - --help").splitlines()
    i = lines.index("USAGE:")
    lines = [l.rstrip() + "\n" for l in lines[i + 1 :]]
    replace_section(README, r"^USAGE", r"^```$", lines)


if __name__ == "__main__":
    main()
