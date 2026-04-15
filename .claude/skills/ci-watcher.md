# CI Watcher

You are a CI monitoring agent. Your ONLY job is to run the bash script below using the Bash tool, then report the result. Do NOT ask questions. Do NOT wait for confirmation. Execute the script immediately.

Run this script now:

```bash
SHA=$(git -C /home/ubuntu/github/tarmo/fastrag rev-parse HEAD)
MAX=40; i=0
while [ $i -lt $MAX ]; do
  api_err=$(mktemp)
  result=$(GH_TOKEN="$GITHUB_PERSONAL_ACCESS_TOKEN" gh api \
    repos/Tarmo-Technologies/fastrag/commits/$SHA/check-runs \
    --jq '.check_runs[] | {name,status,conclusion}' 2>"$api_err")
  if [ -s "$api_err" ]; then
    echo "API error (fatal):"; cat "$api_err"; rm -f "$api_err"; exit 1
  fi
  rm -f "$api_err"
  if [ -n "$result" ]; then
    pending=$(echo "$result" | jq -r 'select(.conclusion == null) | .name' 2>/dev/null)
    if [ -z "$pending" ]; then
      failed=$(echo "$result" | jq -r 'select(.conclusion == "failure") | .name' 2>/dev/null)
      if [ -n "$failed" ]; then
        echo "CI FAILED — failed checks:"; echo "$failed"
        GH_TOKEN="$GITHUB_PERSONAL_ACCESS_TOKEN" gh run list \
          --repo Tarmo-Technologies/fastrag --commit "$SHA" \
          --json databaseId,conclusion \
          --jq '.[] | select(.conclusion == "failure") | .databaseId' 2>/dev/null \
          | while read run_id; do
              GH_TOKEN="$GITHUB_PERSONAL_ACCESS_TOKEN" gh run view "$run_id" \
                --repo Tarmo-Technologies/fastrag --log-failed 2>&1 | tail -60
            done
        exit 1
      fi
      echo "All checks passed:"; echo "$result"; exit 0
    fi
  fi
  i=$((i+1)); sleep 30
done
echo "CI watcher timed out after 20 minutes"; exit 1
```

After the script finishes, report either "CI passed" or "CI failed" with the relevant output.
