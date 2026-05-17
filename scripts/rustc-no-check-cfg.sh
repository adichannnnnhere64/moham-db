#!/usr/bin/env bash
# Strips --check-cfg flags that some Rust versions don't support
args=()
for arg in "$@"; do
    case "$arg" in
        --check-cfg*) ;;
        *) args+=("$arg") ;;
    esac
done
exec rustc "${args[@]}"
