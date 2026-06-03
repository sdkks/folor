#!/usr/bin/env bash
set -euo pipefail

echo "Running pre-commit checks..."

# Secret scanning (skip if gitleaks not installed)
if command -v gitleaks &> /dev/null; then
    echo "Running gitleaks..."
    gitleaks protect --staged --verbose || { echo "Gitleaks found secrets"; exit 1; }
else
    echo "gitleaks not installed, skipping secret scan"
fi

# Formatting check
echo "Checking formatting..."
cargo fmt --check || { echo "Formatting check failed. Run 'make fix'"; exit 1; }

# Lint
echo "Running clippy..."
cargo clippy -- -D warnings || { echo "Clippy warnings found"; exit 1; }

# Tests
echo "Running tests..."
cargo test || { echo "Tests failed"; exit 1; }

# Coverage (skip if cargo-llvm-cov not installed)
if command -v cargo-llvm-cov &> /dev/null; then
    echo "Checking coverage (minimum 80%)..."
    cargo llvm-cov --fail-under-lines 80 --fail-under-functions 80 --ignore-filename-regex '(main\.rs|config\.rs|binary\.rs|file_ref\.rs|signal\.rs|supervisor\.rs|watcher\.rs)' || { echo "Coverage below 80%"; exit 1; }
else
    echo "cargo-llvm-cov not installed, skipping coverage check"
fi

# Build (last — slowest step, skip if earlier checks failed)
echo "Building release..."
cargo build --release || { echo "Release build failed"; exit 1; }

echo "All pre-commit checks passed!"
