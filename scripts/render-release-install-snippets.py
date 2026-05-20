#!/usr/bin/env python3
"""Render README/runbook install snippets from the public release URL contract."""

from __future__ import annotations

import argparse

from release_url_contract import latest_install_command, pinned_install_command, verified_install_handoff_block


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release-tag", default="<version>")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    print(latest_install_command())
    print(pinned_install_command(args.release_tag))
    print()
    print(verified_install_handoff_block(args.release_tag))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
