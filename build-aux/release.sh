#!/bin/bash
# Moves every file that carries the version to a new one, so a release is: run this, open the
# pull request, merge it, push the tag (docs/plans/ci-and-release.md).
#
#   build-aux/release.sh 0.1.3 "First note" "Second note"
#
# The notes are the release's text on GitHub, in the store listing and in the app's “What's New”.
set -euo pipefail
cd "$(dirname "$0")/.."

[ $# -ge 2 ] || { echo "Usage: build-aux/release.sh X.Y.Z \"note\" [\"note\"…]" >&2; exit 1; }
VERSION=$1
shift
[[ $VERSION =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Version must look like 1.2.3" >&2; exit 1; }
CURRENT=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
[ "$(printf '%s\n%s\n' "$CURRENT" "$VERSION" | sort -V | tail -n1)" = "$VERSION" ] && [ "$CURRENT" != "$VERSION" ] \
  || { echo "$VERSION is not newer than $CURRENT" >&2; exit 1; }

export VERSION CURRENT
export DATE=$(date +%F)
export CHANGELOG_DATE=$(LC_ALL=C date "+%a %b %d %Y")
NOTES=$(printf '%s\n' "$@")
export NOTES

python3 - <<'PY'
import os, re
from xml.sax.saxutils import escape

version, current = os.environ["VERSION"], os.environ["CURRENT"]
notes = os.environ["NOTES"].splitlines()


def edit(path, change):
    text = open(path).read()
    new = change(text)
    if new == text:
        raise SystemExit(f"{path}: nothing to change, is it already at {version}?")
    open(path, "w").write(new)


for path in ["Cargo.toml", "crates/core/Cargo.toml", "crates/service/Cargo.toml"]:
    edit(path, lambda t: t.replace(f'version = "{current}"', f'version = "{version}"', 1))
edit("Cargo.lock", lambda t: re.sub(
    r'(name = "tunnelbird(?:-core|-service)?"\nversion = )"%s"' % re.escape(current), rf'\1"{version}"', t))
edit("apple/Resources/Info.plist", lambda t: re.sub(
    r"(CFBundleShortVersionString</key><string>)[^<]*", rf"\g<1>{version}", t, count=1))
edit("build-aux/packaging/tunnelbird.spec", lambda t: re.sub(
    r"(Version:\s+)\S+", rf"\g<1>{version}", t, count=1).replace(
    "%changelog\n",
    "%changelog\n* {} Ferhat Saidoun <ferhatsnpro@gmail.com> - {}-1\n{}\n\n".format(
        os.environ["CHANGELOG_DATE"], version, "\n".join("- " + n for n in notes)), 1))


def listing(text):
    body = "\n".join(f"          <li>{escape(n)}</li>" for n in notes)
    release = (f'    <release version="{version}" date="{os.environ["DATE"]}">\n'
               f"      <description>\n        <ul>\n{body}\n        </ul>\n      </description>\n"
               "    </release>\n")
    return text.replace("  <releases>\n", "  <releases>\n" + release, 1)


edit("data/fr.ferhatsn.Tunnelbird.metainfo.xml", listing)
PY

cargo check --locked --quiet
echo "Release $VERSION is ready (from $CURRENT). Review with git diff, then branch, commit and open the pull request."
