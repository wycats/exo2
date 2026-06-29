#!/usr/bin/env bash
set -euo pipefail

mode="${1:-}"

repo="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
server_url="${GITHUB_SERVER_URL:-https://github.com}"
run_id="${GITHUB_RUN_ID:-}"
run_url="${COVERAGE_RUN_URL:-${server_url}/${repo}/actions/runs/${run_id}}"
audit_branch="${COVERAGE_AUDIT_BRANCH:-automation/rust-coverage-audit}"
audit_label="${COVERAGE_AUDIT_LABEL:-coverage-audit}"
run_label="${COVERAGE_RUN_LABEL:-run-coverage}"
status_context="${COVERAGE_STATUS_CONTEXT:-Rust Coverage Audit}"
marker_path="${COVERAGE_AUDIT_MARKER_PATH:-docs/research/rust-coverage-audit.md}"
dry_run="${COVERAGE_AUDIT_DRY_RUN:-}"

usage() {
  cat >&2 <<'USAGE'
Usage: rust-coverage-audit.sh <mode>

Modes:
  pr-result     Comment on the PR that requested a coverage audit.
  main-result   Maintain the single main coverage tracker PR.
USAGE
}

ensure_labels() {
  gh label create "$run_label" \
    --repo "$repo" \
    --color "0e8a16" \
    --description "Run the optional Rust coverage audit for this PR" \
    >/dev/null 2>&1 || true

  gh label create "$audit_label" \
    --repo "$repo" \
    --color "b60205" \
    --description "Automatically maintained tracker for scheduled coverage failures" \
    >/dev/null 2>&1 || true
}

audit_pr_number() {
  gh pr list \
    --repo "$repo" \
    --state open \
    --head "$audit_branch" \
    --json number \
    --jq '.[0].number // ""'
}

set_audit_status() {
  local sha="$1"
  local state="$2"
  local description="$3"

  gh api \
    --method POST \
    "repos/${repo}/statuses/${sha}" \
    -f "state=${state}" \
    -f "context=${status_context}" \
    -f "target_url=${run_url}" \
    -f "description=${description}" \
    >/dev/null
}

run_checklist_item() {
  local timestamp sha short_sha

  timestamp="${COVERAGE_RUN_TIMESTAMP:-$(date -u +"%Y-%m-%d %H:%M UTC")}"
  sha="${COVERAGE_MAIN_SHA:-$(git rev-parse HEAD 2>/dev/null || echo unknown)}"
  short_sha="${sha:0:12}"

  printf -- '- [ ] [%s main@%s](%s)\n' "$timestamp" "$short_sha" "$run_url"
}

render_failure_body() {
  local pr_number="$1"
  local body_file="$2"
  local item old_body old_items

  item="$(run_checklist_item)"
  old_body="$(mktemp)"
  old_items="$(mktemp)"

  if [[ -n "$pr_number" ]]; then
    gh pr view "$pr_number" --repo "$repo" --json body --jq '.body // ""' >"$old_body"
    grep -E '^- \[ \] \[[^]]+\]\(https://[^)]*/actions/runs/[0-9]+' "$old_body" \
      | grep -Fv "$run_url" >"$old_items" || true
  fi

  {
    cat <<EOF
Rust coverage audit is failing on \`main\`.

This PR is maintained automatically as a tracker for scheduled coverage failures. Use the linked runs to decide what fix to make; fixes should normally land through ordinary PRs.

## Failing Runs

${item}
EOF
    if [[ -s "$old_items" ]]; then
      cat "$old_items"
    fi
    cat <<EOF

## Current Signal

The \`${status_context}\` status on this PR points at the latest failing scheduled run.
EOF
  } >"$body_file"
}

create_or_reset_tracker_branch() {
  git config user.name "github-actions[bot]"
  git config user.email "41898282+github-actions[bot]@users.noreply.github.com"

  git switch -C "$audit_branch"
  mkdir -p "$(dirname "$marker_path")"
  cat >"$marker_path" <<EOF
# Rust Coverage Audit Tracker

This branch is maintained by \`.github/workflows/rust-coverage-audit.yml\`.
The live checklist is kept in the tracker PR body.
EOF

  git add "$marker_path"
  git commit -m "Track Rust coverage audit failure" >/dev/null 2>&1 || true
  git fetch origin \
    "refs/heads/${audit_branch}:refs/remotes/origin/${audit_branch}" \
    >/dev/null 2>&1 || true
  git push --force-with-lease origin "$audit_branch"
}

main_result() {
  local result pr_number body_file pr_sha

  result="${COVERAGE_RESULT:?COVERAGE_RESULT is required}"

  if [[ -n "$dry_run" ]]; then
    if [[ "$result" == "success" ]]; then
      echo "DRY RUN: main coverage passed; would close any open tracker PR."
    else
      body_file="$(mktemp)"
      render_failure_body "" "$body_file"
      echo "DRY RUN: main coverage failed; would create or update tracker PR."
      cat "$body_file"
    fi
    exit 0
  fi

  ensure_labels
  pr_number="$(audit_pr_number)"

  if [[ "$result" == "success" ]]; then
    if [[ -z "$pr_number" ]]; then
      exit 0
    fi

    pr_sha="$(gh pr view "$pr_number" --repo "$repo" --json headRefOid --jq '.headRefOid')"
    set_audit_status "$pr_sha" "success" "main coverage passed"
    gh pr close "$pr_number" \
      --repo "$repo" \
      --delete-branch \
      --comment "Rust coverage audit passed on main: ${run_url}"
    exit 0
  fi

  body_file="$(mktemp)"
  render_failure_body "$pr_number" "$body_file"

  if [[ -z "$pr_number" ]]; then
    create_or_reset_tracker_branch
    gh pr create \
      --repo "$repo" \
      --head "$audit_branch" \
      --base main \
      --title "Rust coverage audit is failing on main" \
      --body-file "$body_file" \
      --label "$audit_label" \
      --label "$run_label"
    pr_number="$(audit_pr_number)"
  else
    gh pr edit "$pr_number" \
      --repo "$repo" \
      --body-file "$body_file" \
      --add-label "$audit_label" \
      --add-label "$run_label"
  fi

  pr_sha="$(gh pr view "$pr_number" --repo "$repo" --json headRefOid --jq '.headRefOid')"
  set_audit_status "$pr_sha" "failure" "main coverage failed; see scheduled run"
}

pr_result() {
  local result pr_number tested_sha body_file comment_id title

  result="${COVERAGE_RESULT:?COVERAGE_RESULT is required}"
  pr_number="${COVERAGE_PR_NUMBER:?COVERAGE_PR_NUMBER is required}"
  tested_sha="${COVERAGE_TESTED_SHA:?COVERAGE_TESTED_SHA is required}"
  body_file="$(mktemp)"

  if [[ "$result" == "success" ]]; then
    title="Rust coverage audit passed"
  else
    title="Rust coverage audit failed"
  fi

  cat >"$body_file" <<EOF
<!-- rust-coverage-audit -->
## ${title}

Run: ${run_url}
Tested SHA: \`${tested_sha}\`
EOF

  if [[ -n "$dry_run" ]]; then
    cat "$body_file"
    exit 0
  fi

  comment_id="$(
    gh api "repos/${repo}/issues/${pr_number}/comments" --paginate \
      --jq '.[] | select(.body | contains("<!-- rust-coverage-audit -->")) | .id' \
      | tail -n 1
  )"

  if [[ -n "$comment_id" ]]; then
    gh api \
      --method PATCH \
      "repos/${repo}/issues/comments/${comment_id}" \
      -f "body=$(cat "$body_file")" \
      >/dev/null
  else
    gh pr comment "$pr_number" --repo "$repo" --body-file "$body_file"
  fi
}

case "$mode" in
  pr-result)
    pr_result
    ;;
  main-result)
    main_result
    ;;
  *)
    usage
    exit 2
    ;;
esac
