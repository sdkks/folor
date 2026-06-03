#!/usr/bin/env bash
set -euo pipefail

commit_msg_file="$1"
commit_msg=$(cat "$commit_msg_file")

# Conventional Commits pattern
pattern='^(feat|fix|docs|style|refactor|perf|test|chore|ci|build|revert)(\([a-zA-Z0-9_-]+\))?!?: .+'

if ! [[ "$commit_msg" =~ $pattern ]]; then
    echo "Invalid commit message format!"
    echo ""
    echo "Expected: <type>(<optional scope>): <description>"
    echo ""
    echo "Types: feat, fix, docs, style, refactor, perf, test, chore, ci, build, revert"
    echo ""
    echo "Examples:"
    echo "  feat: add semantic search"
    echo "  fix(parser): handle empty files"
    echo "  docs: update README"
    echo "  feat!: breaking change"
    echo ""
    echo "Your message: $commit_msg"
    exit 1
fi

echo "Commit message format valid"
