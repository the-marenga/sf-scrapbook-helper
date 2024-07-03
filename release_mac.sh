#!/bin/bash

# This is a helper script to build the two mac releases & generate their
# checksums. This is only for the github release, you should use `cargo run`
# as usual to run this shoftware

version=$(ggrep -oP '(?<=^version = ")[^"]*' Cargo.toml)
targets=("x86_64-apple-darwin" "aarch64-apple-darwin")

mkdir -p dist
for target in "${targets[@]}"; do
  cargo b -r -q --target $target;
  cp target/${target}/release/sf-scrapbook-helper sf-scrapbook-helper
  outfile="sf-scrapbook-helper_v${version}_${target}.zip"
  zip "${outfile}" sf-scrapbook-helper
  rm sf-scrapbook-helper
  sha256sum "${outfile}" > "dist/${outfile}.sha256sum"
  mv "${outfile}" "dist/${outfile}"
done
