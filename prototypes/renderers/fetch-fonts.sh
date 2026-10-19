#!/bin/sh
# Fetches the corpus's fonts, Noto Serif and Noto Sans (SIL Open Font
# License), into target/fonts/, and checks them against the hashes the
# measurements were made with.
set -eu
cd "$(dirname "$0")"
mkdir -p target/fonts
base=https://raw.githubusercontent.com/notofonts/notofonts.github.io/main/fonts
for f in NotoSerif/hinted/ttf/NotoSerif-Regular.ttf NotoSans/hinted/ttf/NotoSans-Regular.ttf NotoSans/hinted/ttf/NotoSans-Bold.ttf; do
    curl -fsSL -o "target/fonts/$(basename "$f")" "$base/$f"
done
cd target/fonts
cat > fonts.sha256 <<'SUMS'
1df075a380fc7cb898acf64c1f7b3b4dd780de3caa860178bf929de35817a913  NotoSans-Bold.ttf
478c558ea716033cd60c03438f628dfa75694dcf6b5f6d505a2f05fd2b4f3823  NotoSans-Regular.ttf
19e72cd8d595fae5bd74a5206f5d938512e1183d4fed7abb1ec1be1d7efa5f88  NotoSerif-Regular.ttf
SUMS
sha256sum -c --quiet fonts.sha256 || shasum -a 256 -c --quiet fonts.sha256
echo "target/fonts/: the corpus's fonts, as measured"
