---
name: Bug report
about: Create a report for a reproducible issue in the Cratera runner
title: "[BUG] "
labels: bug
assignees: ''
---

<!-- IMPORTANT: If this is a security vulnerability or sandbox escape, DO NOT report it publicly here. Please report it privately via https://github.com/cratera-project/cratera/security/advisories/new or contact@cratera.org as outlined in SECURITY.md. -->

### Describe the Bug
A clear and concise description of what the bug is.

### Environment
- **Host OS**: (e.g. Ubuntu 26.04, Fedora 44)
- **Kernel Version**: (`uname -r`)
- **Rust Version**: (`rustc --version`)
- **Firecracker Version**: (`firecracker --version`)
- **Target Language**: (e.g. Rust 2024, Python 3.12, C++20)

### Steps to Reproduce
1. Command run: `...`
2. Request payload: `...`
3. Exact error output / log snippet:

### Did you run `./scripts/ci.sh` locally?
- [ ] Yes, and it passed.
- [ ] No.
