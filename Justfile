set shell := ["powershell", "-NoLogo", "-Command"]

alias fmt := format

default:
    @just --list

format:
    cargo fmt

lint:
    cargo clippy --all-targets --all-features -D warnings

test:
    cargo test --all

check:
    cargo check --workspace

node CONFIG="config/dxid.toml":
    $env:DXID_CONFIG = "{{CONFIG}}"; cargo run -p dxid-node

cli *args:
    cargo run -p dxid-cli -- {{args}}

tui:
    cargo run -p dxid-tui
