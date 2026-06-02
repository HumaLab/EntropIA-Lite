# Tauri Resources

[Español](README.md) | English

This directory contains lightweight resources bundled with the EntropIA Lite desktop app.

## Current Purpose

| Path | Purpose |
| ---- | ------- |
| `lib/pdfium.dll` | Pdfium native library for PDF rendering on Windows x86_64. |
| `lib/libpdfium.so` or `lib/linux-x86_64/libpdfium.so` | Pdfium native library or Linux fixture path for PDF rendering checks. |
| `lib/LICENSE` | License information for bundled native library payloads. |

Pdfium is resolved by the Rust PDF path from bundled resources first, then development resources, then system library paths.

## Lite Boundary

EntropIA Lite does not bundle or require local ML runtime packs. Do not use this directory to document or stage Python, uv, wheelhouse, Paddle/PaddleVL, ONNX Runtime, tokenizers, faster-whisper, spaCy, GGUF, or llama.cpp payloads unless the Lite product scope changes and legal review is updated.

OCR, transcription, LLM, embeddings, and NER are provider-backed in Lite:

- OpenRouter for LLM, embeddings, and NER;
- GLM-OCR/Z.ai for OCR;
- AssemblyAI for transcription.

See the root `README.md`, `PRIVACY.md`, and `THIRD_PARTY_NOTICES.md` for product-level behavior and release review notes.
