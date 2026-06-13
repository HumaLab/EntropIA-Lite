# EntropIA Lite

[Español](README.md) | English

EntropIA Lite is a desktop app for organizing corpora, processing documents, and enriching sources with AI through remote providers. It keeps the EntropIA experience where it makes sense, but removes the heavy local stack: it does not download or run local models for LLMs, embeddings, NER, OCR, or transcription.

Current version: `1.0.3`.

## What's New

This release introduces a conversational chat over your collections: you can now ask EntropIA Lite about the content of your transcriptions and documents and get answers drawn from a hybrid semantic-and-keyword search across your knowledge base. Conversations are saved and can be resumed from a history sidebar. The chat now also draws on text extracted from your documents via OCR, and a new settings tab lets you fine-tune the retrieval parameters. This version further adds a per-collection text analysis panel with a word cloud, and improves how long texts are processed so that embedding generation, entity recognition, and relation extraction are more stable and reliable. Rounding out the release is a set of visual refinements across the interface along with usability fixes, including correct handling of input-method (IME) composition while typing. EntropIA Lite is now also available as a `.deb` package for Linux, in addition to the `.msi` and `.exe` installers for Windows.

## Quick Path

1. Install the app from the internal installer you received.
2. Open Settings and add the API keys for the providers you want to use.
3. Import images, PDFs, or audio.
4. Run OCR, transcription, embeddings, NER, summaries, or triples depending on the configured keys.

Internal installers can be stored locally in `internal-releases/`. That directory is ignored by git and is not part of the source code.

## What It Is

EntropIA Lite is an API-only variant of EntropIA for research and document work. The app keeps a local SQLite database for collections, items, assets, extractions, transcriptions, entities, triples, notes, annotations, FTS, and derived indexes.

The central difference is AI processing:

| Area | Current provider |
| ---- | ---------------- |
| LLM, summaries, correction, triples | OpenRouter |
| Embeddings | OpenRouter, 1024-dimensional BGE-M3 contract |
| NER | OpenRouter |
| OCR for images/PDFs | GLM-OCR / Z.ai |
| Audio transcription | AssemblyAI |

SQLite and the UI remain local. Content processed with remote AI may be sent to the corresponding provider.

## Privacy And Keys

EntropIA Lite does not include first-party telemetry documented in this repo. Workspace data is stored locally, but API-only features send content to external providers when you run them.

OpenRouter, GLM-OCR/Z.ai, and AssemblyAI keys are stored as secret references through the native system keyring, not as raw values intended to persist in the database or logs.

Read also:

- [`PRIVACY.md`](PRIVACY.md)
- [`PRIVACY.en.md`](PRIVACY.en.md)
- [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md)
- [`THIRD_PARTY_NOTICES.en.md`](THIRD_PARTY_NOTICES.en.md)

## Current Scope

Works today for:

- creating collections, items, and assets;
- importing images, PDFs, and audio;
- extracting text through remote OCR;
- transcribing audio through an API;
- enriching with embeddings, NER, summaries, and triples;
- searching through FTS and similarity when embeddings have been generated;
- chatting with your collections through a RAG chat with persistent history;
- analyzing a collection's text with a metrics panel and word cloud;
- reviewing results, metadata, notes, and annotations in the UI.

Out of scope for Lite:

- GGUF/llama.cpp model downloads or execution;
- local Python runtime for ML;
- Paddle, PaddleVL, PaddleOCR, or PaddleX as user dependencies;
- ONNX Runtime, tokenizers, or other local embedding/NER engines;
- local faster-whisper;
- local spaCy;
- self-contained local ML runtime packs.

## Development

Base requirements:

- Node.js 22+
- pnpm 9+
- stable Rust
- WebView2/MSVC on Windows, or equivalent Tauri dependencies on Linux/macOS

Install dependencies:

```bash
pnpm install --frozen-lockfile
```

Run the desktop app:

```bash
pnpm --filter @entropia/desktop tauri dev
```

Isolated dev profile, useful to avoid sharing data with the installed app:

```bash
pnpm desktop:dev:isolated
```

Useful checks, without packaging installers:

```bash
pnpm --filter @entropia/desktop lint
pnpm --filter @entropia/desktop typecheck
pnpm --filter @entropia/desktop test
```

Do not run `tauri build` unless you actually want to generate a local package.

## Key Paths

| Path | Use |
| ---- | --- |
| `apps/desktop/` | Svelte/Tauri desktop app |
| `apps/desktop/src-tauri/` | Rust backend and Tauri configuration |
| `apps/desktop/src-tauri/tauri.conf.json` | `EntropIA Lite` name, version, and bundle identity |
| `apps/desktop/src-tauri/src/settings.rs` | Settings persistence and secret references |
| `apps/desktop/src-tauri/src/ocr/` | Remote OCR through GLM-OCR/Z.ai |
| `apps/desktop/src-tauri/src/transcription/` | Remote transcription through AssemblyAI |
| `apps/desktop/src-tauri/src/llm/` | Remote LLM through OpenRouter |
| `apps/desktop/src-tauri/src/nlp/` | Local FTS, remote embeddings/NER, and enrichment |
| `apps/desktop/src-tauri/resources/` | Lightweight Tauri resources, mainly Pdfium |
| `internal-releases/` | Local internal installers ignored by git |

## Status

EntropIA Lite is at version `1.0.3`. It is usable for API-only document workflows, but should still be treated as beta: UX, data, and provider details may change.
