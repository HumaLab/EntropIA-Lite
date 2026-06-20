# EntropIA Lite

Español | [English](README.en.md)

EntropIA Lite es una app desktop para organizar corpus, procesar documentos y enriquecer fuentes con IA usando proveedores remotos. Conserva la experiencia de EntropIA donde tiene sentido, pero elimina el stack local pesado: no descarga ni ejecuta modelos locales de LLM, embeddings, NER, OCR o transcripción.

Versión actual: `1.0.5`.

## Novedades de esta versión

Lo más importante incorporado desde la última actualización del README:

- **Sincronización cloud multi-dispositivo:** sincronizá tus colecciones entre equipos, con una card de Sincronización en Configuración (preflight y resolución de conflictos) y un indicador de estado en la barra inferior.
- **Centro de notificaciones:** una campana en la barra superior concentra los avisos, incluido el cambio de plan o suscripción.
- **Mapa de entidades geográficas:** las entidades de lugar se geocodifican vía Nominatim/OpenStreetMap y se muestran sobre un mapa dentro del ítem.

La versión `1.0.5` se concentra en estabilizar y endurecer todo eso: robustez y seguridad en la sincronización (entre otras, dejar de filtrar referencias a secretos sin resolver), un límite global de 1 pedido/s contra Nominatim al geocodificar, y mejor contraste de color (WCAG AA) en el tema claro.

EntropIA Lite está disponible como paquete `.deb` para Linux, además de los instaladores `.msi` y `.exe` para Windows.

## Ruta rápida

1. Instalá la app desde el instalador interno que recibiste.
2. Abrí Configuración y cargá las API keys de los proveedores que vas a usar.
3. Importá imágenes, PDFs o audio.
4. Procesá OCR, transcripción, embeddings, NER, resúmenes o triples según las claves configuradas.

Los instaladores internos pueden guardarse localmente en `internal-releases/`. Esa carpeta está ignorada por git y no forma parte del código fuente.

## Qué es

EntropIA Lite es una variante API-only de EntropIA para investigación y trabajo documental. La app mantiene una base local SQLite para colecciones, ítems, assets, extracciones, transcripciones, entidades, triples, notas, anotaciones, FTS e índices derivados.

La diferencia central es el procesamiento de IA:

| Área | Proveedor actual |
| ---- | ---------------- |
| LLM, resúmenes, corrección, triples | OpenRouter |
| Embeddings | OpenRouter, contrato BGE-M3 de 1024 dimensiones |
| NER | OpenRouter |
| OCR para imágenes/PDF | GLM-OCR / Z.ai |
| Transcripción de audio | AssemblyAI |

SQLite y la UI siguen siendo locales. El contenido que proceses con IA remota puede enviarse al proveedor correspondiente.

## Privacidad y claves

EntropIA Lite no incluye telemetría propia documentada en este repo. Los datos de trabajo se guardan localmente, pero las funciones API-only envían contenido a proveedores externos cuando las ejecutás.

Las claves de OpenRouter, GLM-OCR/Z.ai y AssemblyAI se guardan como referencias a secretos mediante el keyring nativo del sistema, no como valores crudos pensados para persistir en la base o logs.

Leé también:

- [`PRIVACY.md`](PRIVACY.md)
- [`PRIVACY.en.md`](PRIVACY.en.md)
- [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md)
- [`THIRD_PARTY_NOTICES.en.md`](THIRD_PARTY_NOTICES.en.md)

## Alcance actual

Funciona hoy para:

- crear colecciones, ítems y assets;
- importar imágenes, PDFs y audio;
- extraer texto por OCR remoto;
- transcribir audio por API;
- enriquecer con embeddings, NER, resúmenes y triples;
- buscar por FTS y similitud cuando hay embeddings generados;
- conversar con tus colecciones mediante un chat RAG con historial persistente;
- analizar el texto de una colección con un panel de métricas y nube de palabras;
- sincronizar tus colecciones entre dispositivos con resolución de conflictos (cloud sync);
- ubicar en un mapa las entidades geográficas detectadas, geocodificadas vía Nominatim/OpenStreetMap;
- revisar resultados, metadata, notas y anotaciones desde la UI.

Fuera de alcance para Lite:

- descarga o ejecución de modelos GGUF/llama.cpp;
- runtime local de Python para ML;
- Paddle, PaddleVL, PaddleOCR o PaddleX como dependencia de usuario;
- ONNX Runtime, tokenizers u otros motores locales de embeddings/NER;
- faster-whisper local;
- spaCy local;
- runtime packs self-contained de ML local.

## Desarrollo

Requisitos base:

- Node.js 22+
- pnpm 9+
- Rust estable
- WebView2/MSVC en Windows, o las dependencias Tauri equivalentes en Linux/macOS

Instalación:

```bash
pnpm install --frozen-lockfile
```

Ejecutar la app desktop:

```bash
pnpm --filter @entropia/desktop tauri dev
```

Perfil dev aislado, útil para no compartir datos con la app instalada:

```bash
pnpm desktop:dev:isolated
```

Checks útiles, sin empaquetar instaladores:

```bash
pnpm --filter @entropia/desktop lint
pnpm --filter @entropia/desktop typecheck
pnpm --filter @entropia/desktop test
```

No ejecutes `tauri build` salvo que realmente quieras generar un paquete local.

## Paths clave

| Path | Uso |
| ---- | --- |
| `apps/desktop/` | App Svelte/Tauri desktop |
| `apps/desktop/src-tauri/` | Backend Rust y configuración Tauri |
| `apps/desktop/src-tauri/tauri.conf.json` | Nombre `EntropIA Lite`, versión e identidad del bundle |
| `apps/desktop/src-tauri/src/settings.rs` | Persistencia de settings y referencias a secretos |
| `apps/desktop/src-tauri/src/ocr/` | OCR remoto vía GLM-OCR/Z.ai |
| `apps/desktop/src-tauri/src/transcription/` | Transcripción remota vía AssemblyAI |
| `apps/desktop/src-tauri/src/llm/` | LLM remoto vía OpenRouter |
| `apps/desktop/src-tauri/src/nlp/` | FTS local, embeddings/NER remotos y enriquecimiento |
| `apps/desktop/src-tauri/resources/` | Recursos Tauri livianos, principalmente Pdfium |
| `internal-releases/` | Instaladores internos locales ignorados por git |

## Estado

EntropIA Lite está en la versión `1.0.5`. Es usable para flujo documental API-only, pero todavía conviene tratarlo como beta: pueden cambiar detalles de UX, datos y proveedores.
