## Description
<!-- Briefly explain what changes this PR introduces and why they are needed. -->

## Changes Made
- 

## Verification & Testing
<!-- Describe the tests you ran to verify this change. -->
- [ ] Ran `./scripts/ci.sh` locally and all 5 checks passed (`fmt`, `clippy`, unit & contract tests, host + musl release builds).
- [ ] Tested microVM execution (`./scripts/smoke.sh`) or added unit/contract tests for any new language or parser logic.

## Contributor Checklist
- [ ] **Diff Review**: I reviewed the entire git diff. I eliminated unnecessary code, premature abstractions, indirection, and unused dependencies.
- [ ] **Accountability**: I personally understand every line of this code and can defend its design and security implications during review.
- [ ] **No Speculative / Hallucinated Code**: All options, arguments, and low-level Linux/KVM mechanics have been verified.
