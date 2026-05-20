#!/usr/bin/env python3
"""Render README/runbook install snippets from the public quickstart contract."""

from __future__ import annotations

import argparse

from quickstart_snippets import install_snippets


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release-tag", default="<version>")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    latest, pinned, verified = install_snippets(args.release_tag)
    print(latest.body)
    print(pinned.body)
    print()
    print(verified.body)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
