#!/bin/sh
# Fetches the General Decimal Arithmetic test cases (Mike Cowlishaw, IBM)
# into target/dectest/. Their license is unclear, so they're fetched,
# never committed (CLAUDE.md, rule 10).
set -eu
cd "$(dirname "$0")"
mkdir -p target/dectest
curl -fsSL -o target/dectest/dectest.zip https://speleotrove.com/decimal/dectest.zip
echo "b70a224cd52e82b7a8150aedac5efa2d0cb3941696fd829bdbe674f9f65c3926  target/dectest/dectest.zip" | sha256sum -c - >/dev/null
cd target/dectest && unzip -oq dectest.zip && echo "target/dectest/: $(ls *.decTest | wc -l) files"
