#!/usr/bin/env sh
set -eu

version="${1:?Usage: scripts/release.sh <version> (e.g. 0.1.0)}"
tag="v$version"

# The version in Cargo.toml is what ends up in the release artifacts
if ! grep -q "^version = \"$version\"$" Cargo.toml; then
  echo "Cargo.toml is not at version $version, bump it first." >&2
  exit 1
fi

# The changelog section for this version becomes the release notes
if ! grep -Eq "^## \[?$version\]?" CHANGELOG.md; then
  echo "CHANGELOG.md has no \"## [$version]\" section, write the release notes first." >&2
  exit 1
fi

git tag "$tag"
git push origin "$tag"

echo "Waiting for the release workflow to start..."
run_id=""
while [ -z "$run_id" ]; do
  sleep 5
  run_id="$(gh run list --workflow release.yml --branch "$tag" --json databaseId --jq '.[0].databaseId // empty')"
done

gh run watch "$run_id" --exit-status

# The formula is attached to the release by now; tell the tap to pull it in, using the local
# gh credentials — the cross-repo trigger the workflows themselves are not allowed to do
gh workflow run sync.yml --repo RobinMalfait/homebrew-tap
echo "Released $tag, the tap is syncing."
