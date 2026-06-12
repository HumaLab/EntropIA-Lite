# Build steps — Linux .deb

## Quick path

El `.deb` NO se compila desde Windows: el bundler de Tauri empaqueta solo para la
plataforma del host (el binario Linux se linkea contra WebKitGTK). El build corre
en GitHub Actions.

1. Workflow: `.github/workflows/build-linux.yml` ("Build Linux (.deb)").
2. Disparadores:
   - Manual: `gh workflow run build-linux.yml` (sube el `.deb` como artifact).
   - Release: al publicar una release de GitHub, el workflow adjunta el `.deb`
     a esa release automáticamente (`gh release upload --clobber`).
3. Artefacto esperado: `apps/desktop/src-tauri/target/release/bundle/deb/EntropIA Lite_<version>_amd64.deb`.

## Details

### Configuración Linux

- `apps/desktop/src-tauri/tauri.linux.conf.json` (merge automático sobre
  `tauri.conf.json` cuando el host es Linux):
  - `targets: ["deb"]`
  - `resources` explícitos: excluye el `pdfium.dll` de Windows (5.7 MB) e incluye
    `resources/lib/linux-x86_64/libpdfium.so`.
  - `deb.depends`: `libdbus-1-3` (keyring), `gstreamer1.0-plugins-base` y
    `gstreamer1.0-plugins-good` (dictado/playback en WebKitGTK).
  - `deb.recommends`: `gnome-keyring` (proveedor de Secret Service).

### PDFium

`resources/lib/linux-x86_64/libpdfium.so` es el binario oficial linux-x64 de
[bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries), build
`chromium/7543` — la MISMA build que `pdfium.dll`. Al actualizar uno, actualizar
el otro a la misma revisión (ver README del directorio).

### Keyring / API keys

`Cargo.toml` declara `keyring` por target: `windows-native` en Windows y
`sync-secret-service` en Linux (D-Bus). Sin un proveedor de Secret Service
corriendo (gnome-keyring, kwallet), las API keys no persisten — por eso el
`recommends`.

### Contrato de CI

`apps/desktop/src-tauri/scripts/tests/build-linux-workflow.Tests.ps1` congela la
estructura del workflow (runner `ubuntu-24.04`, apt deps, node 22, comando de
build, smoke checks `dpkg-deb`, upload). Cualquier cambio al workflow debe
actualizar esa suite primero (TDD).

## Checklist

- [ ] Versión app/base correcta en los tres archivos (compartida con el flujo MSIX).
- [ ] `libpdfium.so` y `pdfium.dll` en la misma build `chromium/<rev>`.
- [ ] Workflow verde (build + smoke checks `dpkg-deb`).
- [ ] `.deb` instalado y probado en una distro real (ícono, OCR PDF, dictado, persistencia de API keys tras reinicio).
