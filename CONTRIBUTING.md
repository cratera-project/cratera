# Contributing to Cratera

Bug reports, suggestions, and pull requests are welcome. For questions or support, open an issue/discussion or contact `contact@cratera.org`.

---

## Prerequisites

- **Linux x86_64** (Ubuntu 24.04 / 26.04, Fedora, Debian, or Arch) with `/dev/kvm` accessible.
  - *macOS / Windows*: Local development and unit tests (`cargo test --workspace`) run natively. MicroVM execution requires a nested VM (OrbStack/Lima/WSL2).
- **Rust 1.97+** (2024 edition) with `x86_64-unknown-linux-musl` target.
- **Docker** (to assemble the multi-language guest root filesystem).

```bash
rustup target add x86_64-unknown-linux-musl
```

---

## Development Setup

```bash
# 1. Download Firecracker binary + guest kernel into ./images
./scripts/fetch-runtime.sh

# 2. Build multi-language guest rootfs in Docker
./scripts/build-rootfs.sh

# 3. Run all checks, lints, unit & contract tests, and release builds
./scripts/ci.sh

# 4. Run microVM smoke test across all 9 supported languages (requires /dev/kvm)
./scripts/smoke.sh
```

---

## Guidelines

- Run `./scripts/ci.sh` before submitting a pull request.
- Keep pull requests focused on a single change, fix, or language addition.
- Add unit and contract tests for parser, validator, language spec, or protocol changes.
- Preserve microVM hardware isolation, non-network boundary (`IPAddressDeny=any`), and jailer UID/GID `20001` permissions.

---

## Use of AI Assistance Policy

AI coding assistants (e.g., Copilot, LLMs) are welcome as productivity tools, subject to the following standards:

1. You are 100% responsible for the correctness, safety, and performance of all code submitted. If you cannot explain the rationale or mechanics of a code change during review, the PR will be closed.
2. Every submission must be tested and verified locally. Never submit speculative, unverified, or hallucinated AI output. All checks (`./scripts/ci.sh`) must pass cleanly before opening a PR.
3. Superficial doc rewrites, hallucinated features, or automated bot PRs without prior issue discussion will be closed without review.
4. Never bypass jailer isolation, seccomp filters, or memory limits based on AI suggestions without validating low-level Linux and KVM mechanics.

### Recommended AI Prompt for Contributors

If you use an AI coding assistant to draft or review code, supply this instruction:

> *"Review the git diff before committing or opening a PR. Treat complexity as technical debt. Aggressively identify unnecessary code, abstractions, indirection, premature optimizations, dead code, and dependencies. Prefer deleting, collapsing, or simplifying over adding layers. Every new abstraction must justify its existence. Suggest the smallest reliable, maintainable solution that satisfies the actual requirements. Do not optimize for hypothetical future needs or architectural purity."*
