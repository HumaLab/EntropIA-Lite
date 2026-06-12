# Build steps — Microsoft Store MSIX (HLab)

## Quick path

1. Verificar identidad exacta de Partner Center:

```text
Name                 = CONICET.EntropIALite
Publisher            = CN=89DF40E5-581A-4120-9A24-F701205485D6
PublisherDisplayName = HLab
```

2. Bumpear la app a la versión base de la release, por ejemplo `1.0.2`:

- `apps/desktop/src-tauri/tauri.conf.json`
- `apps/desktop/src-tauri/Cargo.toml`
- `apps/desktop/package.json`

`Cargo.lock` se arrastra solo al ejecutar el build.

3. Generar el build base de Tauri (EXE + MSI) desde la raíz:

```powershell
pnpm --filter @entropia/desktop tauri build --bundles msi
```

Artefactos esperados:

- `apps/desktop/src-tauri/target/release/entropia-lite-desktop.exe`
- `apps/desktop/src-tauri/target/release/bundle/msi/EntropIA Lite_<version>_x64_en-US.msi`

4. Actualizar la versión hardcodeada en el pipeline de repack (no se deriva de los manifiestos):

- `.tmp/msix-vm/repack-store-msix-on-host.ps1` → `$storeVersion` y la ruta `$output`.
- `.tmp/msix-vm/repack-store-msix-from-good-payload.ps1` → `$storeVersion`, `$hostDest` y `$vmOutput`.
- `.tmp/msix-vm/EntropIALite-StoreTemplate.xml` → `PackagePath`, `Installer Path` (MSI de 3 segmentos) y `Version`.

Si se omite este paso, el repack regenera un MSIX con la versión anterior y Partner Center lo rechaza por versión duplicada.

5. Reempaquetar MSIX de Store desde el host con el pipeline en `.tmp/msix-vm`:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .tmp\msix-vm\repack-store-msix-on-host.ps1
```

El script:

- desempaqueta un MSIX base (por ejemplo `EntropIALite.msix`),
- reescribe `Identity Name`, `Publisher`, `Version` y `PublisherDisplayName` a la identidad Store exacta,
- reemplaza el `entropia-lite-desktop.exe` por el build más reciente,
- reempaqueta el MSIX final en `.tmp/msix-vm\EntropIALite-Store-HLab-<version>.msix`.

6. Validar identidad y checksum del MSIX resultante:

```powershell
Get-FileHash -Algorithm SHA256 `
  ".tmp\msix-vm\EntropIALite-Store-HLab-<version>.msix"
```

7. Subir el `.msix` a **Partner Center**.

## Details

### Versión app/base vs MSIX Store

- Tauri trabaja con `Major.Minor.Build`, por ejemplo `1.0.2`.
- MSIX de Store exige **4 segmentos** `Major.Minor.Build.Revision`.
- **Store rechaza revision distinta de 0**. Por lo tanto:
  - `1.0.2` (Tauri) → `1.0.2.0` (MSIX Store).

Mantener coherencia al bumpear. Si la app base va a `1.0.3`, el MSIX Store va a `1.0.3.0`.

### Scripts relevantes en `.tmp/msix-vm`

| Script | Uso |
| --- | --- |
| `repack-store-msix-on-host.ps1` | Repack host-side. Reempaqueta un MSIX base, aplica identidad Store exacta, reemplaza el `entropia-lite-desktop.exe` con el build actual. |
| `repack-store-msix-from-good-payload.ps1` | Repack dentro de la VM con PowerShell Direct. Útil cuando el host-side falla por permisos o por acceso al SDK de MSIX Packaging Tool. |
| `EntropIALite-StoreTemplate.xml` | Template para MSIX Packaging Tool cuando la conversión se hace con la GUI/CLI en VM. |
| `run-hyperv-msix-*.ps1` | Orquestaciones Hyper-V (background, interactive, IT, resume, detached, from-iso, spike) para regenerar el MSIX base desde MSI vía VM completa. |

Si falta el MSIX base (`EntropIALite.msix`), el camino completo es:

1. generar MSI con Tauri;
2. usar `run-hyperv-msix-background.ps1` o equivalente en una VM Windows completa con la instalación offline de MSIX Packaging Tool;
3. recuperar el MSIX base desde la VM;
4. aplicar `repack-store-msix-on-host.ps1` con la nueva versión.

### Identidad Store exacta

Establecida por Partner Center. No cambiar en ninguna parte del pipeline.

| Campo | Valor |
| --- | --- |
| `Name` | `CONICET.EntropIALite` |
| `Publisher` | `CN=89DF40E5-581A-4120-9A24-F701205485D6` |
| `PublisherDisplayName` | `HLab` |
| `DisplayName` | `EntropIA Lite` |
| `Application Id` | `EntropIALite` |
| `ExecutableName` | `entropia-lite-desktop.exe` |
| `Architecture` | `x64` |

El manifiesto debe validar exactamente estos valores antes de subir a Partner Center.

### Rutas relevantes

- Source code / identidad del build:
  - `apps/desktop/src-tauri/tauri.conf.json`
  - `apps/desktop/src-tauri/Cargo.toml`
  - `apps/desktop/package.json`
- Pipeline de packaging:
  - `.tmp/msix-vm/repack-store-msix-on-host.ps1`
  - `.tmp/msix-vm/repack-store-msix-from-good-payload.ps1`
  - `.tmp/msix-vm/EntropIALite-StoreTemplate.xml`
  - `.tmp/msix-vm/run-hyperv-msix-*.ps1`
- Artefactos generados:
  - `apps/desktop/src-tauri/target/release/entropia-lite-desktop.exe`
  - `apps/desktop/src-tauri/target/release/bundle/msi/EntropIA Lite_<version>_x64_en-US.msi`
  - `.tmp/msix-vm/EntropIALite-Store-HLab-<version>.msix`

### Release `1.0.3.0` ya verificada

El MSIX `1.0.3.0` fue producido con el repack host-side y verificado contra manifiesto:

- Ruta: `.tmp/msix-vm/EntropIALite-Store-HLab-1.0.3.0.msix`
- Tamaño: 8,435,525 bytes
- SHA256: `CD7E425D325E435B88922FA2D2ACF12A96DEF4A78DA7C9088D7181F82963154E`
- Payload: `entropia-lite-desktop.exe` 1.0.3 (13,634,048 bytes), hash idéntico al build de release
- Identidad leída del manifiesto:
  - `Name` = `CONICET.EntropIALite`
  - `Publisher` = `CN=89DF40E5-581A-4120-9A24-F701205485D6`
  - `Version` = `1.0.3.0`
  - `PublisherDisplayName` = `HLab`
  - `DisplayName` = `EntropIA Lite`

### Release `1.0.2.0` ya verificada

Para referencia, el MSIX `1.0.2.0` ya fue producido y verificado contra manifiesto:

- Ruta: `.tmp/msix-vm/EntropIALite-Store-HLab-1.0.2.0.msix`
- Tamaño: 8,268,230 bytes
- SHA256: `3A50FED8B4FDBCB792F661F24D7E55E71AA4F1AC1BB72427F42E80449494AB9D`
- Identidad leída del manifiesto:
  - `Name` = `CONICET.EntropIALite`
  - `Publisher` = `CN=89DF40E5-581A-4120-9A24-F701205485D6`
  - `Version` = `1.0.2.0`
  - `PublisherDisplayName` = `HLab`
  - `DisplayName` = `EntropIA Lite`

## Checklist

- [ ] Identidad Store confirmada contra Partner Center.
- [ ] Versión app/base bumpeada en los tres archivos.
- [ ] Versión hardcodeada actualizada en los scripts de repack y el template (`.tmp/msix-vm`).
- [ ] `tauri build --bundles msi` finalizado OK.
- [ ] MSIX base disponible (host) o generado vía VM completa.
- [ ] `repack-store-msix-on-host.ps1` ejecutado OK.
- [ ] Identidad del manifiesto revalidada (Name / Publisher / Version / PublisherDisplayName).
- [ ] SHA256 del MSIX final guardado en el checklist de release.
- [ ] Artefacto subido a Partner Center.

## Next step

- Subir `.tmp\msix-vm\EntropIALite-Store-HLab-<version>.msix` a Partner Center.
- Si Partner Center rechaza la versión, validar que el cuarto segmento (`Revision`) sea `0` y que la identidad coincida exactamente.
