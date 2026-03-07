#!/bin/bash
set -e

# Check if cargo-llvm-cov is installed
if ! cargo llvm-cov --version &> /dev/null; then
  echo "cargo-llvm-cov is not installed. Installing..."
  cargo install cargo-llvm-cov
fi

# Run coverage and generate HTML report
echo "Running tests with coverage..."
cargo llvm-cov --all-features --workspace --html
cargo llvm-cov --all-features --workspace # Print text summary

echo "Coverage report generated at target/llvm-cov/html/index.html"

# Open the report if on macOS
if [[ "$OSTYPE" == "darwin"* ]]; then
  open target/llvm-cov/html/index.html
fi