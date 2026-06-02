# Recursos Tauri

Español | [English](README.en.md)

Este directorio contiene recursos livianos incluidos con la app desktop EntropIA Lite.

## Propósito actual

| Path | Propósito |
| ---- | --------- |
| `lib/pdfium.dll` | Librería nativa Pdfium para renderizar PDFs en Windows x86_64. |
| `lib/libpdfium.so` o `lib/linux-x86_64/libpdfium.so` | Librería nativa Pdfium o ruta fixture de Linux para checks de renderizado PDF. |
| `lib/LICENSE` | Información de licencia para payloads nativos incluidos. |

La ruta PDF de Rust resuelve Pdfium primero desde recursos incluidos, después desde recursos de desarrollo y finalmente desde paths de librería del sistema.

## Límite de Lite

EntropIA Lite no incluye ni requiere runtime packs locales de ML. No uses este directorio para documentar o preparar payloads de Python, uv, wheelhouse, Paddle/PaddleVL, ONNX Runtime, tokenizers, faster-whisper, spaCy, GGUF o llama.cpp salvo que cambie el alcance del producto Lite y se actualice la revisión legal.

OCR, transcripción, LLM, embeddings y NER usan proveedores en Lite:

- OpenRouter para LLM, embeddings y NER;
- GLM-OCR/Z.ai para OCR;
- AssemblyAI para transcripción.

Ver el `README.md`, `PRIVACY.md` y `THIRD_PARTY_NOTICES.md` de la raíz para comportamiento de producto y notas de revisión de release.
