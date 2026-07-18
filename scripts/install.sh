#!/bin/sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
project_dir=$(dirname "$script_dir")
app_install_dir=${HISTORIES_APP_INSTALL_DIR:-"$HOME/Applications"}
cli_install_dir=${HISTORIES_CLI_INSTALL_DIR:-"$HOME/.local/bin"}

cd "$project_dir"

pnpm install --frozen-lockfile
cargo test --workspace
pnpm tauri build
cargo build --release -p histories-cli

mkdir -p "$app_install_dir" "$cli_install_dir"
ditto "$project_dir/target/release/bundle/macos/Histories.app" "$app_install_dir/Histories.app"
install -m 755 "$project_dir/target/release/histories" "$cli_install_dir/histories"

printf 'Installed Histories.app to %s\n' "$app_install_dir/Histories.app"
printf 'Installed histories CLI to %s\n' "$cli_install_dir/histories"
