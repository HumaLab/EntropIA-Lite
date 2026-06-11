# Auditoría y Plan — EntropIA Lite

**Fecha:** 2026-06-11 · **Alcance:** monorepo completo (frontend Svelte 5 + Vite, backend Tauri 2/Rust, packages compartidos) · **Método:** auditoría multi-agente en 7 dimensiones (rendimiento de edición de imagen, pipeline OCR, calidad TypeScript, calidad Rust, UX/estética, configuración/tests, seguridad) con verificación adversarial doble de cada hallazgo (un agente intenta refutarlo, otro evalúa la seguridad del fix) antes de aplicar nada. Todos los cambios quedaron en el working tree, **sin commits, sin ramas remotas, sin push**.

---

## 1. Resumen ejecutivo

Se auditaron ~64.000 líneas fuente (46k TS/Svelte, 17.5k Rust). El proyecto está en buen estado general: arquitectura clara por módulos, 705 tests TS y 142 tests Rust en línea base, operaciones de imagen ya corrían en `spawn_blocking` y con archivos versionados para undo. Pero la línea base **fallaba 5 de 7 gates de calidad**: lint (1 error), typecheck (23 errores, 20 de ellos enmascarados porque turbo cancela el typecheck de `desktop` cuando falla `ui`), `cargo fmt` (13 diffs), `cargo clippy -D warnings` (37 warnings) y `cargo test` (1 test de chunking fallando).

La auditoría produjo **62 hallazgos altos/medios verificados** (30 aplicables con seguridad, 32 reales pero diferidos) más 27 menores. Se aplicaron **52 mejoras** (los 30 confirmados + fixes de línea base + 5 correcciones de la revisión adversarial post-cambio + extras triviales). Estado final: **todos los gates verdes** — lint, typecheck (0 errores en 5.170 archivos), 723/723 tests TS (+18 nuevos), build Vite, `cargo fmt --check`, `clippy -D warnings` (0 warnings) y 153/153 tests Rust (+12 nuevos).

Los problemas más serios encontrados y corregidos: cliente HTTP de GLM-OCR **sin timeout** (un request colgado congelaba la cola OCR para siempre), `render_pdf_pages` **re-inicializaba Pdfium y re-parseaba el PDF entero por cada página**, `ocr:complete` se emitía aunque fallara el guardado en DB, comandos `db_*` síncronos bloqueando el runtime async, fuga sistémica de listeners Tauri en los 5 stores del frontend, documentos pdf.js nunca destruidos (fuga de memoria del worker), y contraste WCAG insuficiente en el tema claro.

Lo más importante que queda pendiente (decisión consciente, no descuido): **CSP deshabilitado** (`csp: null`), validación de paths en comandos IPC, paralelización del worker OCR, y la inexistencia de CI (que el propio test suite del repo exige).

---

## 2. Línea base vs. estado final

| Gate | Línea base | Final |
|---|---|---|
| `pnpm lint` | ❌ 1 error (no-regex-spaces) | ✅ limpio |
| `pnpm typecheck` (turbo) | ❌ 3 errores en ui + cancelado desktop | ✅ 0 errores |
| `svelte-check` desktop directo | ❌ 20 errores (enmascarados por turbo) | ✅ 0 errores / 5.170 archivos |
| `pnpm test:run` | ✅ 705/705 | ✅ 723/723 (+18) |
| `pnpm build` | ✅ (warning de chunk >500 kB) | ✅ (warning persiste, ver pendientes) |
| `cargo fmt --check` | ❌ 13 hunks en 5 archivos | ✅ limpio |
| `cargo clippy --all-targets -- -D warnings` | ❌ 37 warnings | ✅ 0 warnings |
| `cargo test` | ❌ 1 fallo (`nlp::chunking`) | ✅ 153/153 (+12, 2 ignored pre-existentes que requieren Pdfium) |

---

## 3. Cambios aplicados

### 3.1 Rendimiento — edición de imagen (`src-tauri/src/image_edit.rs`)
- **Crop sin triple copia:** se eliminó el canvas RGBA8 intermedio + `copy_from`; `crop_imm` ya devuelve una imagen own con el color type original. Menos memoria pico (~3 copias full-size → 1) y sin conversión RGBA→RGB redundante en el encoder.
- **Calidad JPEG explícita (92):** antes cada edición re-encodeaba JPEG a calidad 75 (default del crate `image`), degradando la imagen generacionalmente con cada crop/rotación/borrado. Ahora hay un helper `save_image` con `JpegEncoder::new_with_quality(92)`.
- **`erase_region` sin promoción a RGBA:** los rellenos opacos sobre fuentes RGB8/RGBA8 se hacen in-place sobre el buffer existente, con escritura por filas (`chunks_exact_mut`) en lugar de `put_pixel` por píxel con bounds-check. La conversión a PNG por transparencia sobre JPEG se conserva intacta.
- +4 tests nuevos que fijan dimensiones, versionado, ruta JPEG con calidad y no-promoción RGBA.

### 3.2 Rendimiento y robustez — pipeline OCR (`src-tauri/src/ocr/`)
- **Pdfium una sola vez por documento:** nueva `render_pdf_pages_to_png_files()` que bindea el engine y parsea el PDF una vez y renderiza todas las páginas en loop (antes: re-init + re-parse completo **por página**). Contrato IPC, nombres de archivo y shape de retorno intactos.
- **Timeouts en GLM-OCR:** `connect_timeout(15s)` + `timeout(300s)`. Antes un request colgado dejaba la cola OCR congelada para siempre.
- **Persistencia transaccional:** extracción + layout se guardan en una transacción; si falla el guardado se emite `ocr:error` y **ya no** un `ocr:complete` falso. Con tests de atomicidad y rollback.
- **`busy_timeout=5000`** en la conexión del worker OCR (y se extendió a todas las conexiones escritoras: ver 3.4).
- **Sanitización de `filename_prefix`** (path traversal): separadores, `..`, `:` y caracteres de control se neutralizan; fallback a `document`. +3 tests.

### 3.3 Rendimiento — frontend (DocumentViewer, ItemView, stores)
- **Ciclo de vida pdf.js correcto:** `loadingTask.destroy()`/`pdfDoc.destroy()` al cambiar de asset, al cambiar de modo y al desmontar — antes el worker de pdf.js acumulaba documentos sin liberar entre assets. El `$effect` de carga ahora trackea `assetUrl` (antes solo `type`: cambiar entre dos PDFs mostraba el documento viejo) y tiene guard de staleness. `pdfDoc`/`activeRenderTask` tipados con `PDFDocumentProxy`/`RenderTask` (eran los únicos `any` de producción).
- **Guard anti-respuesta-vieja en `ItemView.loadData`:** un token de request descarta respuestas que no sean la última (antes, navegar rápido entre ítems podía pintar datos del ítem equivocado).
- **Fuga sistémica de listeners Tauri cerrada:** en los 5 stores (`ocr.ts`, `nlp.ts`, `transcription.ts`, `llm.ts`, `geo.ts`), si `stopListening()` corría antes de que resolvieran los `listen()` de `startListening()`, los listeners quedaban registrados para siempre. Ahora hay flag de generación/cancelación con unlisten inmediato de registros tardíos, con tests en cada store (incluye `geo.test.ts` nuevo).
- **Crop con dimensiones frescas:** la región de crop se calculaba con las dimensiones naturales viejas hasta que la imagen nueva terminara de cargar; ahora se actualizan desde el resultado del comando Rust.

### 3.4 Calidad y robustez — Rust transversal
- **Comandos `db_*` async:** los 7 comandos de `db/commands.rs` ahora corren el trabajo rusqlite dentro de `tokio::task::spawn_blocking` (antes bloqueaban el thread del runtime con SQL sincrónico). Nombres, argumentos y retornos idénticos.
- **Timeouts HTTP en todos los clientes:** OpenRouter 120s/20s, AssemblyAI 60s/20s (upload 600s), Nominatim 30s/15s. Polling de AssemblyAI acotado (~60 min) con error descriptivo al expirar.
- **Sin panics en workers:** construcción de clientes reqwest en geo y NER pasó de `unwrap/expect` a degradación con error (el perfil release usa `panic=abort`: un panic mataba la app entera en silencio). Se eliminó el constructor `OpenRouterClient::new()` que paniqueaba; todo usa `try_new()`.
- **`busy_timeout=5000` en todas las conexiones SQLite escritoras:** ui_conn y worker_conn (lib.rs), workers de geo/transcription/llm (worker + job), backfill de embeddings (nlp). Con WAL y 6+ conexiones concurrentes, antes cualquier contención devolvía `SQLITE_BUSY` inmediato.
- **Triples streaming:** el texto completo del documento se lee una vez antes del loop de chunks (antes: una lectura SQLite por chunk).
- **Settings/keyring sin bloquear el runtime:** operaciones de Windows Credential Manager envueltas en `spawn_blocking`.
- **Dependencias muertas eliminadas** de Cargo.toml: `pdf-extract`, `ed25519-dalek`, `zip`, `thiserror` (verificadas sin referencias).
- **Base64 hand-rolled reemplazado** por el crate `base64` ya presente (salida byte-idéntica, con test de regresión).
- **Búsqueda del DB browser escapa wildcards LIKE** (`%`, `_`, `\` + cláusula `ESCAPE`): buscar `100%` ya no matchea cualquier cosa.
- **Test de chunking corregido:** el test fallante estaba mal construido (su fixture de 25.101 chars no superaba `MAX_CHARS=28.000`, así que nunca ejercitaba el escenario del nombre). La implementación era correcta según su contrato documentado. Fixture reescrito para probar de verdad que un `\n` en la segunda mitad de la ventana gana sobre uno en la primera.
- `cargo fmt` aplicado + 37 warnings de clippy resueltos (auto-fix + 5 manuales; un solo `#[allow(too_many_arguments)]` justificado en `NlpQueue::start_worker`).

### 3.5 UX y accesibilidad
- **ConfirmDialog:** foco inicial en Cancelar, focus trap (Tab/Shift+Tab), Escape cierra de forma confiable, restauración de foco al invocador. +4 tests.
- **Ctrl+B ya no secuestra Bold:** el toggle global de sidebar ignora eventos desde inputs, textareas, selects y contenteditable (el editor de notas tiptap usa Ctrl+B para negrita). Con test.
- **Contraste WCAG AA en tema claro:** `--text-muted` pasó de 2.85:1 a 5.33:1 y `--text-secondary` de 4.48:1 a 4.54:1 (y se ajustó el tema oscuro a ≥4.69:1), manteniendo la familia de matiz.
- **Búsqueda global del TopBar usable por teclado:** ArrowUp/Down con wrap, Enter activa, Escape cierra; los resultados ya no se destruyen por el timeout de blur antes de poder clickearlos (focusout con `relatedTarget`); semántica combobox/listbox/option con ARIA completo. +3 tests. *Nota:* el rol del input cambió de `searchbox` a `combobox` — tests futuros deben consultar ese rol.
- **Settings i18n:** las pestañas "Prompts" y "Model Params" (títulos, botones, mensajes de validación, links de API keys) dejan de estar hardcodeadas en español — 23 claves nuevas en `i18n.ts` (es + en), redacción española byte-idéntica a la anterior. Con test de cobertura de locales.
- **Labels del editor de metadatos en ItemView** ahora usan las claves i18n existentes en lugar de un objeto hardcodeado en español.
- **Errores de persistencia visibles:** `DebouncedAnnotationPersistor` acepta `onError` (espejo de su hermano de texto) y `ItemView` lo cablea a `console.error`; los `startListening()` fire-and-forget de onMount tienen `.catch`.
- Código muerto eliminado: `DebouncedAssetReanalysisScheduler`, derived `filtered` no-op en CollectionView, parámetro `hasOcrText` nunca usado en `selectOcrCorrectionAssetId`, JSDoc engañoso de `extractText`.

### 3.6 Configuración y tests
- **`.gitignore` ya no se ignora a sí mismo** y quedó **staged** (`git add`, sin commit): los clones frescos ahora reciben las reglas de ignore.
- **turbo.json:** `typecheck` y `test` ya no dependen de `^build` (se verificó con probe que svelte-check no necesita el dist de `ui`: sus exports apuntan a fuente). Esto además elimina el enmascaramiento que ocultó 20 errores de typecheck. Se borró la task muerta `rust:quality:report`, el output fantasma `.svelte-kit/**` y los outputs de `test` que generaban warnings.
- **`vitest.workspace.ts` (deprecado) migrado** a `vitest.config.ts` con `test.projects` (vitest 3.2.4).
- **vite.config.ts:** gate de minify/sourcemap pasó de `TAURI_DEBUG` (variable de Tauri v1, nunca seteada por Tauri 2) a `TAURI_ENV_DEBUG`.
- **tsconfig raíz** ahora extiende `./packages/config-ts/base.json` (única fuente de verdad de strictness; gana `noUncheckedIndexedAccess` y `forceConsistentCasingInFileNames`).
- `engines: { node: ">=22" }` en package.json raíz; `jsdom` (no usado, los tests corren happy-dom) removido de devDependencies.
- **Errores de typecheck corregidos:** tipos literales de prompts en SettingsView (anotación `$state<string>`), 6 `<Input type="number">` inválidos → `type="text"` (ver riesgos), guards en SettingsView.test.ts y AudioPlayer.test.ts, mock de items-repo en CollectionView.test.ts completado con `findCardSummariesByCollection` sin romper el feature-detection del componente, guard de null en AnnotationToolbar, regex de AppShell.test.ts.

---

## 4. Qué NO se hizo y por qué (hallazgos reales diferidos)

Verificados como reales por los agentes, pero **no auto-aplicables** sin decisión humana o trabajo mayor. Ordenados por prioridad sugerida:

### Seguridad (requieren diseño, no son triviales)
1. **CSP deshabilitado (`csp: null`) en tauri.conf.json** — el gap de seguridad más importante. Definir una CSP que permita `asset:`/blob/data para imágenes y pdf.js requiere prueba manual en el webview real; un error rompe la app entera. *Próximo paso #1.*
2. **Comandos IPC leen/escriben paths arbitrarios** sin canonicalización ni scope-check (`path_utils.rs`) — necesita una capa central de validación de paths.
3. **El renderer tiene autoridad SQL completa** vía `db_execute`/`db_execute_batch` genéricos — reducirlo a repos tipados es un refactor de arquitectura.
4. **Capability de fs muy amplia** (lectura recursiva de home/desktop/documents/downloads) — acotarla puede romper flujos de import existentes; revisar con casos de uso reales.
5. `{@html}` en notas depende de un sanitizador propio sin respaldo de CSP (mitigado parcialmente: el sanitizador es sólido; DOMPurify recomendado) y `validate_sql_batch` valida por substring (sobre-bloquea literales legítimos).
6. `transcribe_dictation` borra cualquier path que le pase el caller; `deleteWithCascade` interpola IDs con escape manual (hoy UUIDs internos, no explotable).

### Rendimiento (cambios de comportamiento o gran alcance)
7. **Worker OCR estrictamente secuencial** — paralelizar jobs GLM independientes (network-bound) es la mejora de throughput más grande disponible, pero cambia semántica de orden y carga.
8. **Pdfium se bindea de cero en cada operación** (thumbnails, probes, renders) — un singleton/cache compartido es el complemento natural del fix 3.2.
9. Import de PDF escaneado renderiza todas las páginas en un invoke opaco — sin progreso ni cancelación.
10. Rotación fina convierte permanentemente a PNG RGBA con ~5x memoria pico; las ediciones no reusan la imagen decodificada entre operaciones consecutivas; los archivos versionados de undo nunca se recolectan (fuga de disco).
11. Archivos enteros a base64 en memoria para GLM-OCR sin guard de tamaño; cliente reqwest nuevo por chunk NER/embedding; upload AssemblyAI bufferea el audio completo en RAM; `similar_assets` escanea hasta 2000 embeddings con el mutex de UI tomado; dedup NER se libera antes de que el job termine (jobs remotos duplicados posibles).
12. **Leaflet y tiptap van estáticos al chunk principal de 1 MB** (pdfjs ya está code-split) — lazy-load o `manualChunks`; probar en el webview.

### UX (necesitan decisión de producto)
13. **El import auto-navega** y el resumen de errores parciales nunca se ve.
14. El panel derecho resetea a "Notes" en cada cambio de página y tras cada edición de imagen (`$effect` keyed por identidad de objeto).
15. Escape global navega hacia atrás en medio de tareas (no cancela edición de imagen; descarta ediciones de Settings en silencio).
16. TopBar (labels de tema, controles de ventana) y errores de CollectionView siguen hardcodeados en español/inglés; anotaciones del DocumentViewer son mouse-only (`tabindex=-1`).
17. Startup con 16 `.expect()` encadenados: con `panic=abort` la app muere sin diálogo ante cualquier fallo de migración.

### Tests y CI
18. **No existe CI** — y el propio test PowerShell del repo (`ci-rust-quality-workflow.Tests.ps1`) afirma que `.github/workflows/ci.yml` debe existir. Crear el workflow (lint+typecheck+test+build+clippy) es el paso con mejor relación costo/beneficio del backlog.
19. ESLint no lintea archivos `.svelte` (70% del frontend sin cobertura de lint) — agregar `eslint-plugin-svelte` implica triage de hallazgos nuevos.
20. Tests de migraciones del store asercionan substrings SQL contra un mock no-op; ~2.500 líneas Rust de providers remotos y 8 de 13 vistas sin tests; `packages/store/src/migrations/*.sql` es un duplicado stale del registro real inline.
21. `fts_search` registrado como comando IPC es un stub que devuelve un JSON placeholder.

---

## 5. Riesgos detectados (de los cambios aplicados y del proceso)

- **Inputs de Model Params pasaron de `type="number"` a `type="text"`:** el union `InputType` del componente compartido nunca soportó `number`, y restaurarlo habría hecho que el `bind:value` de Svelte coercionara a `number` un estado tipado `string` (rompiendo la validación `.trim()`). Se pierde el spinner nativo; la validación manual existente cubre el resto. Mejora correcta futura: soporte `number` real en el componente `Input` con estado numérico.
- **Labels de clave enmascarada en Settings** se traducen al cargar, no reaccionan a un cambio de idioma en vivo (igual que antes del cambio); labels de parámetros (`maxTokens (1-32000, vacío = default)`) siguen en español — están clavados en tests con aserciones literales.
- **Rol ARIA del buscador global cambió** (`searchbox` → `combobox`) y los resultados son `option` — cualquier test/tooling externo que consultara los roles viejos debe actualizarse (los del repo ya están).
- **Incidente durante la aplicación paralela:** un agente ejecutó `git reset --hard HEAD` a mitad de proceso (visible en el reflog), revirtiendo transitoriamente ediciones de otros. Se recuperó todo y el estado final fue re-verificado end-to-end (los 7 gates + revisión adversarial del diff completo), pero si notás algo raro en el reflog, esa es la causa.
- **`pnpm-lock.yaml`** conserva una entrada peer-opcional de jsdom (decisión deliberada para no re-resolver el lockfile entero).
- **2 tests Rust `#[ignore]` pre-existentes** dependen del binario Pdfium en runtime (no se ejecutan en `cargo test`).
- **Verificación funcional:** se hizo vía suites completas (723 tests TS + 153 Rust, muchos de integración sobre vistas con Testing Library) + builds; **no** se ejecutó la app Tauri viva con interacción manual. Antes de release: smoke test manual del flujo principal (import → OCR → editar imagen → notas → export), en especial OCR de PDF multipágina y crop/rotación sobre JPEG.

---

## 6. Próximos pasos recomendados (en orden)

1. **CSP**: diseñar y probar una Content-Security-Policy real en el webview (con `asset:`, blob y worker de pdf.js). Es el ítem de seguridad #1.
2. **CI**: workflow de GitHub Actions con los 7 gates (ya quedan todos verdes — es el momento ideal para congelarlos).
3. **Smoke test manual** del flujo principal sobre este working tree antes de commitear.
4. **Paralelizar el worker OCR** + singleton de Pdfium (mayor ganancia de rendimiento restante).
5. **Validación central de paths IPC** y reducción del SQL genérico del renderer.
6. **eslint-plugin-svelte** + triage.
7. UX: resumen de import visible, no resetear el tab del panel derecho, Escape no destructivo.
8. Garbage collection de versiones de imagen para undo (fuga de disco acotable con una política simple de retención).

---

## 7. Comandos usados (verificación)

```bash
pnpm lint && pnpm typecheck && pnpm test:run && pnpm build      # raíz del repo
cd apps/desktop && pnpm typecheck                                # svelte-check directo (desenmascarado)
cd apps/desktop/src-tauri
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

## 8. Archivos modificados (resumen)

- **Rust (24):** `image_edit.rs`, `ocr/{mod,commands,pdf,glm_ocr,postprocess}.rs`, `db/commands.rs`, `llm/{mod,commands,openrouter,prompt}.rs`, `nlp/{mod,commands,chunking,embeddings,fts,ner/mod,ner/openrouter}.rs`, `transcription/{mod,commands,assemblyai}.rs`, `geo/mod.rs`, `settings.rs`, `lib.rs`, `Cargo.toml`/`Cargo.lock`
- **Frontend desktop (27):** `ItemView.svelte`, `SettingsView.svelte`, `CollectionView.svelte`, `AppShell.svelte`, `TopBar.svelte`, `lib/{ocr,nlp,transcription,llm,geo,i18n,item-view-*}.ts` + tests (incl. `geo.test.ts` nuevo), `vite.config.ts`, `package.json`
- **packages/ui (7):** `DocumentViewer.svelte`, `ConfirmDialog.svelte`, `AnnotationToolbar.svelte`, `tokens.css` + tests
- **Config (7):** `turbo.json`, `tsconfig.json`, `package.json`, `.gitignore` (staged), `vitest.config.ts` (nuevo), `vitest.workspace.ts` (eliminado), `pnpm-lock.yaml`

*Total: 66 archivos modificados, ~1.820 inserciones / ~1.010 eliminaciones. Nada commiteado: revisá con `git diff` y commiteá por unidades de trabajo cuando estés conforme.*
