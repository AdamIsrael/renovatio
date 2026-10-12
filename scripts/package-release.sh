#!/bin/bash
# Package a release build into dist/: a tarball laid out to extract into
# ~/.local or /usr/local, and its SHA256SUMS.
#
#   cargo build --workspace --release
#   scripts/package-release.sh v0.1.0
#
# With --check, only check that the tag matches the Cargo.toml version.
set -euo pipefail

check_only=false
if [ "${1:-}" = "--check" ]; then
    check_only=true
    shift
fi
tag=${1:?usage: $0 [--check] vX.Y.Z}
cd "$(dirname "$0")/.."

# The version is part of the plugin ABI fingerprint, so the tag must match it.
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
if [ "$tag" != "v$version" ]; then
    echo "Tag $tag doesn't match the version in Cargo.toml ($version)" >&2
    exit 1
fi
if $check_only; then
    exit 0
fi

name="renovatio-$tag-$(uname -m)-linux"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

install -Dm755 target/release/renovatio "$stage/bin/renovatio"
mkdir -p "$stage/lib/renovatio/plugins"
# libtest_plugin.so is development-only, as in `just install-release`
plugins=$(find target/release -maxdepth 1 -name '*.so' ! -name 'libtest_plugin.so' | sort)
if [ -z "$plugins" ]; then
    echo "No plugins in target/release; run cargo build --workspace --release" >&2
    exit 1
fi
for plugin in $plugins; do
    install -m755 "$plugin" "$stage/lib/renovatio/plugins/"
done
install -Dm644 README.md "$stage/share/doc/renovatio/README.md"

mkdir -p dist
tar -czf "dist/$name.tar.gz" -C "$stage" --owner=0 --group=0 bin lib share
(cd dist && sha256sum "$name.tar.gz" > SHA256SUMS)
echo "dist/$name.tar.gz"
tar -tzf "dist/$name.tar.gz"
