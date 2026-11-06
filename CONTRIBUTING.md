# Contributing to Kanga OS

This document outlines the strict guidelines and standards for submitting code to the Kanga kernel. We adhere to a rigid, data-driven engineering process inspired directly by the upstream [Linux Kernel Documentation](https://docs.kernel.org/process/1.Intro.html "Linux Kernel Process Guide"). 

---

## I. Code Quality & Architectural Principles

Kanga is an independent, experimental x86-64 kernel milestone written in `no_std` Rust. We maintain absolute separation from standard user-space paradigms.

1. **No Scope Creep:** All patches must tightly align with active milestone goals (e.g., core VMM layout or low-level task scheduling primitives). Speculative features or premature high-level abstractions will be aggressively rejected.
2. **Explicit Over Implicit:** Prefer explicit low-level interfaces and explicit matching over complex high-level trait veneers. Code must clearly demonstrate how it manipulates CPU states, page tables, or hardware MMIO boundaries.
3. **Rust Memory Safety:** Minimize the use of `unsafe` blocks. Any unavoidable memory manipulation must contain a descriptive, audited safety comment explaining why a safe alternative cannot satisfy the hardware constraints.

---

## II. Patch & Commit Message Requirements

Every code submission must conform to the canonical patch format to allow efficient review and processing via command-line tooling.

1. **Imperative Subject Lines:** Write your commit summary in the present-tense imperative mood (e.g., `x86/power: Fix kernel panic during sleep resume`). Keep the subject line under 50 characters, followed by a clear subsystem prefix matching the repository module layout.
2. **Data-Driven Descriptions:** Your commit message body must explicitly describe *what* is currently broken, *why* it fails, and *how* your code resolves the issue. Back up your assertions with real-world trace data, benchmarks, or QEMU log assertions.
3. **The Developer Certificate of Origin (DCO):** To certify that you have the legal right to contribute your code under our GPL-2.0 license, you must explicitly append a `Signed-off-by` tag to the bottom of every commit. Submissions missing this signature block will be dropped instantly.

   ```text
   Signed-off-by: Your Name <your.email@example.com>
   ```

---

## III. Review Etiquette & The Veto System

All incoming patch traffic is tracked asynchronously on the project repository. Reviews are objective, direct, and hyper-focused on engineering stability.

1. **Inline Communication:** When responding to peer or maintainer reviews, strictly use inline bottom-posting. Quote only the specific code snippets under critique, strip out unnecessary conversational filler, and place your technical response immediately beneath.
2. **The Veto Levels:** Submissions will be triaged using explicit routing codes based on technical merit:
   * **ACK (Acknowledgement):** Code is vetted, verified to be correct, and queued for a merge window.
   * **Level 1 NAK:** Minor Request for Changes. Code requires restructuring or re-formatting before reconsidering.
   * **Level 2 NAK (Broken by Design):** The architectural pattern causes regressions, lock contentions, or concurrency race hazards. Requires a complete rethink of the design path.
   * **Level 3 NAK (Brain-Dead):** Code violates fundamental computer science principles, breaks core baseline constraints, or was posted without compiling.

---

## IV. Testing & Validation Pipeline

Before submitting a patch series or opening a pull request, your local workspace must pass our mandatory automated validation criteria:

1. **Automated Formatting:** Run `cargo fmt` to enforce explicit workspace indentation styles.
2. **Linting Verification:** Ensure your branch triggers zero errors or warnings when evaluated via standard compilation rules.
3. **Hardware Emulation:** Validate your artifact output boot path inside QEMU. While QEMU stability serves as our base validation metric, patches touching critical clock cycles or hardware configuration fallback logic must also pass validation traces on real x86-64 hardware profiles before final integration.
