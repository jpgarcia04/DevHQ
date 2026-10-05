#!/usr/bin/env bash
# Decides whether the Windows jobs run for this pull request event, and writes windows=true or
# windows=false to $GITHUB_OUTPUT.
#
# A `paths` filter on `pull_request` compares the whole pull request with its base, so once a
# pull request touches src-tauri/ every push to it runs Windows again. Here the comparison
# starts at the newest other commit of the branch on which every Windows job ran and passed:
# what changed since then is what Windows has not tested yet. A failed or cancelled run never
# counts, so whatever it did not finish is tested by the next run, and a re-run of a commit
# compares from the same place as its first run did. Without such a commit (the first push of a
# pull request) it compares with where the branch left its base, as the paths filter did.
#
# Environment: HEAD, BASE (the pull request's head and base commits), BRANCH and
# HEAD_REPOSITORY (where the head lives), GITHUB_REPOSITORY and GH_TOKEN; run from a clone that
# has the whole history. Any doubt widens the comparison to the whole pull request, so a skipped
# job never hides an untested change.
set -euo pipefail

# What the Windows jobs test: the native app, its IPC contract, the native walk, this workflow
# and its scripts, and the npm lockfile, which pins the Tauri CLI that builds the NSIS installer.
TESTED='^(src-tauri/|contract/|e2e/native/|\.github/workflows/windows\.yml$|\.github/windows/|package-lock\.json$)'

decide() {
  echo "windows=$1" >> "$GITHUB_OUTPUT"
  {
    echo "### Windows jobs: $([ "$1" = true ] && echo run || echo skipped)"
    echo
    echo "$2"
  } >> "${GITHUB_STEP_SUMMARY:-/dev/null}"
  echo "$2"
}

baseline=""
unlisted=""
runs=$(gh api --method GET "repos/$GITHUB_REPOSITORY/actions/workflows/windows.yml/runs" \
  -f branch="$BRANCH" -f status=success -F per_page=50 \
  --jq '.workflow_runs[] | "\(.id) \(.head_sha) \(.head_repository.full_name)"') ||
  runs="" unlisted="GitHub did not list the earlier runs. "
# The runs come newest first.
while read -r run sha repository; do
  [ -n "$run" ] || continue
  # Another fork can have a branch with the same name; a re-run of this commit is no baseline.
  [ "$repository" = "$HEAD_REPOSITORY" ] && [ "$sha" != "$HEAD" ] || continue
  # A run whose Windows jobs were skipped also succeeded: only one in which every job
  # succeeded tested its commit.
  unfinished=$(gh api "repos/$GITHUB_REPOSITORY/actions/runs/$run/jobs?per_page=100" \
    --jq '[.jobs[] | select(.conclusion != "success")] | length') || continue
  [ "$unfinished" = 0 ] || continue
  # After a force push the commit may be on no branch, but GitHub may still serve it.
  git cat-file -e "$sha^{commit}" 2>/dev/null ||
    git fetch --quiet --no-tags origin "$sha" 2>/dev/null || continue
  baseline=$sha
  since="Every Windows job passed on $baseline (run $run)."
  break
done <<< "$runs"

if [ -z "$baseline" ]; then
  if ! baseline=$(git merge-base "$BASE" "$HEAD"); then
    decide true "No commit to compare with: every Windows job runs."
    exit 0
  fi
  since="${unlisted:-No earlier commit of this branch passed every Windows job. }Comparing with $baseline, where the branch left its base."
fi

files() { # "1 file", "2 files"
  [ "$1" = 1 ] && echo "1 file" || echo "$1 files"
}
changed=$(git diff --name-only "$baseline" "$HEAD")
tested=$(grep -E "$TESTED" <<< "$changed" || true)
others=$(grep -cvE "$TESTED|^$" <<< "$changed" || true)
if [ -n "$tested" ]; then
  count=$(grep -c . <<< "$tested")
  listed=$(head -n 15 <<< "$tested" | sed 's/^/- /')
  [ "$count" -le 15 ] || listed="$listed
- and $((count - 15)) more"
  decide true "$since Since then, $(files "$count") that Windows tests changed:
$listed"
else
  decide false "$since Since then, nothing Windows tests changed: only $(files "$others") that it does not test."
fi
