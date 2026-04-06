---
name: Project Goal
description: aztec-rust is a Rust SDK for Aztec Network, modeled after starknet-rs with aztec.js API coverage
type: project
---

Building a Rust SDK for the Aztec Network in /Users/alexmetelli/source/aztec-rust.

**Why:** No Rust SDK exists for Aztec. The user wants incremental, minimal-first delivery.

**How to apply:**
- Reference /Users/alexmetelli/source/starknet-rust for Rust SDK design patterns (traits, builders, workspace layout)
- Reference /Users/alexmetelli/source/aztec-packages/yarn-project/aztec.js for the target API surface
- SPEC.md in the repo root contains the full specification
- Implementation phases: types -> providers -> signers/accounts -> contracts -> fees/events -> polish
