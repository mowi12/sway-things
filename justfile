# SPDX-FileCopyrightText: 2026 Sway Things contributors
# SPDX-License-Identifier: GPL-3.0-or-later

# Run every check, like CI
check:
    #!/usr/bin/env bash
    set -uo pipefail
    failed=()
    for script in scripts/check-*.sh; do
        echo "==> $script"
        "$script" || failed+=("$script")
    done
    if ((${#failed[@]})); then
        printf 'failed: %s\n' "${failed[@]}" >&2
        exit 1
    fi

# Format Rust, Markdown, TOML, YAML and JSON files
fmt:
    scripts/check-fmt.sh --fix

# Build the binaries, the merged metadata and the translations for PREFIX (default /usr/local)
build *args:
    scripts/build.sh {{ args }}

# Install into DESTDIR/PREFIX (run `just build` first)
install:
    scripts/install.sh

# Remove what `just install` installed from DESTDIR/PREFIX
uninstall:
    scripts/install.sh --uninstall
