# Spike de publicación Microsoft Store / MSIX

Objetivo: validar si EntropIA Lite puede publicarse por Microsoft Store usando un paquete MSIX firmado por Store, para ofrecer un canal público de instalación sin advertencias de SmartScreen para usuarios finales.

Este spike NO reemplaza las pre-releases técnicas de GitHub. GitHub sigue siendo útil para testers; Microsoft Store sería el canal público normal cuando la app esté lista para distribución seria.

## Decisión a validar

| Pregunta | Respuesta esperada |
| --- | --- |
| ¿Podemos evitar SmartScreen para usuarios finales? | Sí, si la instalación ocurre desde Microsoft Store con paquete MSIX/AppX firmado por Microsoft. |
| ¿Sirve subir el EXE/MSI actual a Store? | No como solución completa: el flujo EXE/MSI de Store exige instalador firmado, URL versionada y silent install. |
| ¿Tauri genera MSIX Store nativo hoy? | No asumirlo. Tauri v2 documenta Store vía EXE/MSI; por eso este spike prueba conversión MSIX. |
| ¿Qué build usar? | Build normal no-Dev. Nunca `desktop:build:isolated` para Store pública. |

## Alcance del spike

Validar técnicamente:

- conversión del instalador normal de EntropIA Lite a MSIX;
- arranque de la app dentro del paquete MSIX;
- funcionamiento de datos locales, recursos y proveedores remotos;
- compatibilidad con Windows App Certification Kit;
- viabilidad de submission en Partner Center.

Fuera de alcance en este spike:

- publicar globalmente en Store;
- comprar certificados OV/EV;
- automatizar CI/CD de Store;
- cambiar el canal GitHub Releases.

## Preflight del repo

Antes de ejecutar el spike, confirmar:

- [ ] Repo limpio: `git status --short --branch`.
- [ ] Build normal apunta a `EntropIA Lite`, no `EntropIA Lite Dev`.
- [ ] `apps/desktop/src-tauri/tauri.conf.json` usa `identifier: "com.entropia.lite"`.
- [ ] No se usa `apps/desktop/src-tauri/tauri.dev.conf.json`.
- [ ] `PRIVACY.md` y `PRIVACY.en.md` están actualizados.
- [ ] Las notas públicas de release/listing se redactan en español.

## Fase 1 — Build normal no-Dev

Ejecutar desde la raíz del repo:

```powershell
pnpm --filter @entropia/desktop lint
pnpm --filter @entropia/desktop typecheck
pnpm --filter @entropia/desktop test
pnpm --filter @entropia/desktop tauri build
```

Artefactos esperados:

- `apps/desktop/src-tauri/target/release/bundle/nsis/EntropIA Lite_0.1.0_x64-setup.exe`
- `apps/desktop/src-tauri/target/release/bundle/msi/EntropIA Lite_0.1.0_x64_en-US.msi`

Regla crítica: si aparece `EntropIA Lite Dev`, se usó el build equivocado.

## Fase 2 — Conversión MSIX

Herramienta candidata: MSIX Packaging Tool.

Preparar una VM Windows completa y limpia, idealmente con snapshot/checkpoint, para evitar capturar ruido del sistema sin depender de Windows Sandbox.

Pasos:

1. Instalar MSIX Packaging Tool.
2. Crear paquete desde instalador MSI o EXE normal.
3. Usar metadata consistente:
   - Package display name: `EntropIA Lite`
   - Publisher display name: el publisher real de Partner Center
   - Version: `0.1.0.0` o equivalente Store-compatible
   - Architecture: x64
4. Instalar el MSIX resultante localmente.
5. Registrar cualquier fixup necesario.

Riesgo principal: MSIX puede restringir o alterar accesos que el instalador tradicional permite. No asumir compatibilidad sin pruebas reales.

### Resultado del intento con Windows Sandbox

Windows Sandbox no queda recomendado como entorno de conversión para este spike.

Hallazgos del intento:

- La VM Sandbox inicia correctamente y ejecuta scripts como administrador.
- MSIX Packaging Tool 1.2024.405.0 se instala dentro del Sandbox.
- La CLI lee el template de conversión y reconoce el MSI normal de EntropIA Lite.
- La conversión falla antes de capturar porque DISM no logra instalar `Msix.PackagingTool.Driver~~~~0.0.1.0`.
- El error persiste incluso usando el paquete FOD offline oficial `Msix-PackagingTool-Driver-Package-amd64.cab` y `/ScratchDir:C:\DismScratch`.

Conclusión: para avanzar con MSIX hace falta una VM Windows completa, no Sandbox. Si el driver de captura no instala en la VM completa, recién ahí descartar MSIX Packaging Tool como camino viable.

## Fase 3 — Matriz de validación runtime

Validar en el MSIX instalado:

- [ ] La app abre sin crash.
- [ ] La ventana custom chrome funciona.
- [ ] El directorio de datos local se crea correctamente.
- [ ] SQLite persiste colecciones, ítems y notas.
- [ ] Importar imagen funciona.
- [ ] Importar PDF funciona y Pdfium carga.
- [ ] Importar audio funciona.
- [ ] Settings guarda y recupera referencias de secretos.
- [ ] OpenRouter test connection funciona con credencial de prueba.
- [ ] AssemblyAI test connection funciona con credencial de prueba.
- [ ] Z.ai/GLM-OCR test connection funciona con credencial de prueba.
- [ ] OCR remoto genera texto.
- [ ] STT remoto genera transcripción.
- [ ] INDEX FTS se ejecuta automáticamente cuando corresponde.
- [ ] EMBED se ejecuta solo en los flujos definidos.
- [ ] NER sigue siendo manual.
- [ ] Links externos abren navegador del sistema sin `cmd.exe`.
- [ ] Desinstalación limpia.

No imprimir claves ni logs con secretos. Verificar presencia/longitud, nunca valores.

## Fase 4 — Windows App Certification Kit

Ejecutar Windows App Certification Kit sobre el paquete MSIX.

Aceptar el spike solo si:

- [ ] WACK no marca errores bloqueantes.
- [ ] Los warnings están documentados y tienen resolución o justificación.
- [ ] La app sigue siendo funcional después de instalar desde MSIX.

## Fase 5 — Preparación Partner Center

Antes de crear la submission:

- [ ] Definir cuenta: Individual o Company.
- [ ] Reservar nombre `EntropIA Lite`.
- [ ] Confirmar publisher visible.
- [ ] Definir mercados y precio.
- [ ] Completar clasificación por edad/IARC.
- [ ] Agregar URL pública de privacidad.
- [ ] Preparar descripción Store en español.
- [ ] Preparar screenshots.
- [ ] Declarar uso de IA generativa cuando aplique.
- [ ] Incluir mecanismo/canal de reporte para contenido problemático de IA.
- [ ] Agregar notas para certificación explicando proveedores remotos configurados por el usuario.

## Copy base para Store en español

Descripción corta candidata:

> EntropIA Lite organiza corpus documentales y permite procesar imágenes, PDFs y audio con proveedores remotos de IA configurados por el usuario.

Descripción funcional:

> EntropIA Lite es una app desktop para crear colecciones, importar fuentes documentales y enriquecerlas con OCR, transcripción, búsqueda textual, embeddings, entidades, resúmenes y triples. La base de trabajo se guarda localmente en la máquina del usuario. Las funciones de IA remota solo se ejecutan cuando el usuario configura sus propias claves de proveedor y dispara esos flujos desde la app.

Features candidatas:

- Organización de corpus y colecciones documentales.
- Importación de imágenes, PDFs y audio.
- OCR remoto para imágenes y PDFs.
- Transcripción remota de audio.
- Búsqueda FTS local y similitud por embeddings.
- Resúmenes, correcciones y triples mediante LLM remoto.
- Configuración de claves propias para OpenRouter, AssemblyAI y Z.ai.

## Criterio de éxito

El spike se considera exitoso si:

- se genera un MSIX instalable de EntropIA Lite normal, no Dev;
- el paquete pasa validación técnica local suficiente;
- WACK no bloquea;
- la app mantiene datos locales, recursos nativos y proveedores remotos funcionando;
- Partner Center acepta el paquete en una submission inicial o identifica solo ajustes resolubles.

## Criterio de descarte

Descartar o pausar este camino si:

- MSIX rompe acceso a recursos críticos;
- Pdfium o dependencias nativas no cargan de forma confiable;
- Credential Manager/keyring no funciona dentro del paquete;
- WACK devuelve errores estructurales difíciles de corregir;
- Partner Center exige EXE/MSI firmado en vez de aceptar MSIX.

## Alternativas si MSIX falla

| Alternativa | Tradeoff |
| --- | --- |
| Azure Artifact Signing / Trusted Signing | Costo bajo mensual, pero requiere identidad elegible y configuración Azure. |
| OV code signing | Más directo para GitHub, pero SmartScreen puede tardar en ganar reputación. |
| EV code signing | Mejor reputación inicial, más caro y más burocrático. |
| GitHub Releases sin firma | Útil para testers técnicos, no ideal para usuarios finales. |

## Próximo paso recomendado

Ejecutar la Fase 2 en una VM Windows completa con snapshot/checkpoint, registrar resultados y decidir si avanzamos a WACK y Partner Center.
