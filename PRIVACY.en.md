# EntropIA Lite Privacy Notice

[Español](PRIVACY.md) | English

EntropIA Lite is a desktop app with local data storage and remote AI providers. Your workspace data lives on your machine, but OCR, transcription, LLM, embeddings, and NER features send the selected content to external APIs when you run them.

## Local Data

| Data | Handling |
| ---- | -------- |
| Collections, items, metadata, notes, annotations | Stored in the local app database. |
| Imported assets | Referenced or copied according to the desktop import flow. |
| OCR text, transcriptions, entities, triples, summaries, FTS and vectors | Stored locally after provider processing completes. |
| Operational logs | Written locally for diagnostics. Review logs before sharing them. |

## Remote Processing

| Feature | Provider | What may be sent |
| ------- | -------- | ---------------- |
| LLM tasks, summaries, corrections, triples | OpenRouter | Prompt text and relevant document context. |
| Embeddings | OpenRouter | Text to vectorize. |
| NER | OpenRouter | Text to analyze for entities. |
| OCR | GLM-OCR / Z.ai | Selected image or PDF content. |
| Transcription | AssemblyAI | Selected audio content. |

The current Lite profile does not advertise or require local model downloads for GGUF/llama, Paddle/PaddleVL, ONNX/tokenizers, faster-whisper, or spaCy.

## API Keys

OpenRouter, GLM-OCR/Z.ai, and AssemblyAI keys are user-provided secrets. The app stores secret references in local settings and resolves the actual values through the native system keyring when available.

Treat keys as credentials:

- do not commit app data, local databases, logs, or settings snapshots;
- review diagnostics before sharing them;
- rotate any key that may have been exposed.

## User Control

- Do not configure a provider key if you do not want that provider used.
- Remove a provider key from Settings to disable that remote path.
- Delete the local app data directory if you want to remove local databases, logs, generated outputs, and settings references.

## Cloud Sync (optional)

Multi-device sync is **opt-in**: nothing travels to the server until you log into a sync account. If you never enable it, this section does not apply and the rest of the app works the same, fully local.

When you do enable it, note:

| Topic | Detail |
| ----- | ------ |
| What travels | The rows of the 15 synced tables (collections, items, assets, notes, annotations, OCR extractions, transcriptions, layouts, entities, triples, topics, item-topic links, LLM results, and RAG conversations/messages) plus the associated files (images, rendered PDFs, audio). **What does NOT travel**: `app_settings`, vector embeddings, FTS, and the image undo history (`_vN`). |
| No end-to-end encryption | The server **sees** your data. In-transit protection is TLS (HTTPS required except for `localhost`); there is no E2E in v1. Only sync against a server you control or trust. |
| Conflict journal | On a concurrent edit, resolution is "last write wins" per row. The **losing version is kept in full** in a local conflict journal so nothing is silently lost and you can review it. That losing payload stays in your local database until you delete it or sign out. |
| Blob persistence | Uploaded files **persist on the server until you delete the account**. There is no automatic garbage collection: deleting an asset locally does not remove its blob from the server. |
| Account deletion | "Delete my server data" (with password confirmation) removes rows, conflicts, blob metadata, counters, and devices, and deletes your account's blob directory. Your **local** data stays intact. |
| Server logs | The server records the account and device (id, not the token) per request for diagnostics and observability. |
| Operator backups | If the server operator runs backups (Litestream for the database, restic/rclone for blobs), those backups **retain deleted data until they rotate**. Account deletion does not purge replicas or backup snapshots. |
| Device token | Each login creates a new device with an opaque token stored **only in the OS keyring**. The token is never logged and never stored in the database. You can revoke devices from the app. |

## Provider Terms

Remote providers have their own privacy policies, retention rules, and account controls. Review OpenRouter, Z.ai/GLM-OCR, and AssemblyAI terms before processing sensitive material.
