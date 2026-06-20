# EntropIA Lite

[Español](README.md) | English

EntropIA Lite is a desktop app for organizing corpora, processing documents, and enriching sources with AI through remote providers. It keeps the EntropIA experience where it makes sense, but removes the heavy local stack: it does not download or run local models for LLMs, embeddings, NER, OCR, or transcription.

Current version: `1.0.5`.

## What's New

The most important additions since the README was last updated:

- **Multi-device cloud sync:** sync your collections across devices, with a Sync card in Settings (preflight and conflict resolution) and a status indicator in the bottom bar.
- **Notification center:** a bell in the top bar gathers alerts, including plan or subscription changes.
- **Map of geographic entities:** place entities are geocoded via Nominatim/OpenStreetMap and shown on a map within the item.

Version `1.0.5` focuses on stabilizing and hardening all of the above: sync robustness and security fixes (among them, no longer leaking unresolved secret references), a global 1 request/s limit against Nominatim when geocoding, and improved color contrast (WCAG AA) in the light theme.

EntropIA Lite is available as a `.deb` package for Linux, in addition to the `.msi` and `.exe` installers for Windows.

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
- syncing your collections across devices with conflict resolution (cloud sync);
- placing detected geographic entities on a map, geocoded via Nominatim/OpenStreetMap;
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

EntropIA Lite is at version `1.0.5`. It is usable for API-only document workflows, but should still be treated as beta: UX, data, and provider details may change.
