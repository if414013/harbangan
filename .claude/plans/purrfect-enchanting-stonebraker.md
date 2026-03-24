# Remove `backend/benches/` Directory

## Status: DONE (changes applied, pending commit)

## Context

The `backend/benches/` folder contained a single Cargo benchmark harness file (`benchmark.rs`) gated behind a `bench` feature flag. Not part of normal builds, tests, or CI. Separate from `backend/src/bench/` which is an in-app benchmarking toolkit compiled into the binary.

## Changes Applied

1. Deleted `backend/benches/` directory
2. Removed from `backend/Cargo.toml`:
   - `bench = ["hdrhistogram"]` feature
   - `hdrhistogram` optional dependency
   - `[[bench]]` target section
3. No doc updates needed

## Verification

`cargo clippy --all-targets` passed clean (pre-existing warnings only).
