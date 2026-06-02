# Third-Party Notices

[Español](THIRD_PARTY_NOTICES.md) | English

This file is a human-readable checklist for EntropIA Lite. It is not a generated SBOM.

EntropIA Lite depends on standard Rust/Node desktop dependencies, Pdfium for PDF rendering, and external AI providers. It should not be reviewed as a self-contained local-ML runtime distribution.

## Release Rule

Before publishing or sharing an installer, verify:

- [ ] Rust dependency licenses are acceptable for redistribution.
- [ ] pnpm/Node dependency licenses are acceptable for redistribution.
- [ ] Bundled native libraries are versioned and license-reviewed.
- [ ] Installer hashes are recorded for the release handoff.
- [ ] Provider-facing documentation states that OCR, transcription, LLM, embeddings, and NER use remote APIs.
- [ ] No local ML runtime payload is bundled or advertised unless a future change explicitly reintroduces and reviews it.

## Current Components To Review

| Component | Purpose | Current path or source | Review note |
| --------- | ------- | ---------------------- | ----------- |
| Tauri 2 / Rust crates | Desktop shell and backend | `apps/desktop/src-tauri/Cargo.toml` | Review Cargo dependency licenses. |
| Svelte/Vite/pnpm packages | Frontend app | `package.json`, `apps/desktop/package.json`, `pnpm-lock.yaml` | Review npm dependency licenses. |
| Pdfium | PDF rendering | `apps/desktop/src-tauri/resources/lib/` | Keep version/license trace for bundled `pdfium.dll`/`libpdfium.so`. |
| OpenRouter | LLM, embeddings, NER | User-configured API | User must review provider terms for submitted text. |
| GLM-OCR / Z.ai | OCR | User-configured API | User must review provider terms for submitted image/PDF content. |
| AssemblyAI | Audio transcription | User-configured API | User must review provider terms for submitted audio. |

## Explicitly Not Part Of Lite

These components may appear in legacy names, tests, or historical context, but they are not part of the advertised Lite runtime:

- GGUF/llama.cpp local LLM downloads;
- Paddle, PaddleVL, PaddleOCR, PaddleX local OCR/runtime packs;
- ONNX Runtime or tokenizers for local embeddings/NER;
- faster-whisper local transcription;
- spaCy local NER;
- Python/uv/wheelhouse runtime packs for local ML.

If any of those are reintroduced, update this notice and produce the appropriate license review before distribution.

## SBOM Expectation

A formal release should attach or generate an SBOM that covers:

- Cargo dependencies;
- pnpm dependencies;
- bundled native libraries;
- installer metadata and hashes.

Remote API providers are service dependencies, not bundled artifacts, but their terms should remain visible to users.
