# Avisos de terceros

Español | [English](THIRD_PARTY_NOTICES.en.md)

Este archivo es una lista de revisión legible para EntropIA Lite. No es un SBOM generado.

EntropIA Lite depende de dependencias desktop estándar de Rust/Node, Pdfium para renderizar PDFs y proveedores externos de IA. No debe revisarse como una distribución autocontenida de runtime local de ML.

## Regla de release

Antes de publicar o compartir un instalador, verificá:

- [ ] Las licencias de dependencias Rust son aceptables para redistribución.
- [ ] Las licencias de dependencias pnpm/Node son aceptables para redistribución.
- [ ] Las librerías nativas incluidas tienen versión y revisión de licencia.
- [ ] Los hashes del instalador quedan registrados para la entrega del release.
- [ ] La documentación orientada a proveedores indica que OCR, transcripción, LLM, embeddings y NER usan APIs remotas.
- [ ] No se incluye ni publicita payload local de ML salvo que un cambio futuro lo reintroduzca y revise explícitamente.

## Componentes actuales a revisar

| Componente | Propósito | Path o fuente actual | Nota de revisión |
| ---------- | --------- | -------------------- | ---------------- |
| Tauri 2 / crates Rust | Shell desktop y backend | `apps/desktop/src-tauri/Cargo.toml` | Revisar licencias de dependencias Cargo. |
| Paquetes Svelte/Vite/pnpm | App frontend | `package.json`, `apps/desktop/package.json`, `pnpm-lock.yaml` | Revisar licencias de dependencias npm. |
| Pdfium | Renderizado de PDFs | `apps/desktop/src-tauri/resources/lib/` | Mantener trazabilidad de versión/licencia para `pdfium.dll`/`libpdfium.so`. |
| OpenRouter | LLM, embeddings, NER | API configurada por el usuario | El usuario debe revisar términos del proveedor para el texto enviado. |
| GLM-OCR / Z.ai | OCR | API configurada por el usuario | El usuario debe revisar términos del proveedor para imágenes/PDFs enviados. |
| AssemblyAI | Transcripción de audio | API configurada por el usuario | El usuario debe revisar términos del proveedor para audio enviado. |

## Explícitamente fuera de Lite

Estos componentes pueden aparecer en nombres legacy, tests o contexto histórico, pero no forman parte del runtime Lite publicitado:

- descargas locales de LLM GGUF/llama.cpp;
- Paddle, PaddleVL, PaddleOCR, PaddleX o runtime packs locales de OCR;
- ONNX Runtime o tokenizers para embeddings/NER locales;
- transcripción local con faster-whisper;
- NER local con spaCy;
- runtime packs Python/uv/wheelhouse para ML local.

Si alguno de esos componentes se reintroduce, actualizá este aviso y realizá la revisión de licencias correspondiente antes de distribuir.

## Expectativa de SBOM

Un release formal debería adjuntar o generar un SBOM que cubra:

- dependencias Cargo;
- dependencias pnpm;
- librerías nativas incluidas;
- metadata y hashes del instalador.

Los proveedores de APIs remotas son dependencias de servicio, no artefactos incluidos, pero sus términos deben seguir visibles para los usuarios.
