# Cratera Project Governance

Cratera is a hardware-isolated code execution and judge engine written in Rust, powered by Firecracker microVMs.

This document outlines the governance model, decision-making process, scope discipline, and long-term open-source commitments of the Cratera project.

---

## 1. Core Mission & Scope Discipline

The mission of Cratera is singular and non-negotiable:

> **Provide a fast, unescapable, self-hostable code execution sandbox and judge engine in a single Rust binary.**

To maintain simplicity, reliability, and security, Cratera enforces strict scope discipline:
- Firecracker microVM lifecycle management, Jailer containment, `vsock` IPC, declarative language runtimes (`languages.toml`), microsecond telemetry and memory profiling (`RssAnon`), guest agents, and a lean HTTP coordinator.
- Web UIs/dashboards, user authentication systems, distributed database clustering, queue backends, and multi-cloud orchestration. Cratera is designed to be the foundational execution layer embedded into higher-level platforms (such as coding contest sites, auto-graders, CTF runners, and CI tools).

---

## 2. Governance Model

Cratera currently operates under a Benevolent Dictator for Life (BDFL) model:

- Rustu (`contact@cratera.org`) leads technical direction, releases, and architectural decisions.
- Significant changes (e.g. protocol alterations, major API redesigns, new sandbox containment mechanisms) are proposed and discussed openly via GitHub Issues and Discussions prior to implementation.
- Community contributions and bug fixes are reviewed against the standards outlined in [CONTRIBUTING.md](CONTRIBUTING.md).

---

## 3. Perpetual Open-Source & Anti-Relicensing Commitment

Trust is the foundation of infrastructure software. The project operates under the following principles:

1. Cratera is and will remain open-source under the **Apache-2.0** license. There will never be a relicense for the project under BSL (Business Source License), SSPL (Server Side Public License), or restrictive open-core models.
2. All vulnerability patches, security enhancements, and isolation hardening are published directly to the open-source codebase simultaneously with any release.
3. Core execution capabilities, language support, snapshot restoration, and Jailer integrations are 100% accessible to every self-hoster without commercial licensing tiers.

---

## 4. Security & Vulnerability Handling

Security disclosures take highest priority. Vulnerabilities and sandbox escape vectors must be reported privately in accordance with [SECURITY.md](SECURITY.md) via GitHub Private Vulnerability Reporting or `contact@cratera.org`.

Every validated security report receives an immediate triage response within 12 hours and a coordinated remediation release.
