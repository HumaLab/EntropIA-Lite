Set-StrictMode -Version Latest

Describe "build-linux deb workflow contract" {
  BeforeAll {
    function Assert-True {
      param(
        [bool]$Condition,
        [string]$Message
      )

      if (-not $Condition) {
        throw $Message
      }
    }

    function Assert-Match {
      param(
        [string]$Value,
        [string]$Pattern,
        [string]$Message
      )

      if ($Value -notmatch $Pattern) {
        throw $Message
      }
    }

    $script:TestRoot = if ($PSScriptRoot) { $PSScriptRoot } else { Split-Path -Parent $MyInvocation.MyCommand.Path }
    $script:RepoRoot = (Resolve-Path -Path (Join-Path $script:TestRoot "../../../../..")).Path
    $script:workflowPath = Join-Path -Path $script:RepoRoot -ChildPath ".github/workflows/build-linux.yml"
  }

  It "defines the Build Linux (.deb) workflow with dispatch and release triggers" {
    Assert-True -Condition (Test-Path -Path $script:workflowPath) -Message "build-linux workflow file must exist at .github/workflows/build-linux.yml"

    $workflow = Get-Content -Path $script:workflowPath -Raw

    Assert-Match -Value $workflow -Pattern "name:\s*Build Linux \(\.deb\)" -Message "workflow must be explicitly named Build Linux (.deb)"
    Assert-Match -Value $workflow -Pattern "on:\s*[\s\S]*?workflow_dispatch:" -Message "workflow must support manual workflow_dispatch trigger"
    Assert-Match -Value $workflow -Pattern "on:\s*[\s\S]*?release:\s*[\s\S]*?types:\s*(\[\s*published\s*\]|[\s\S]*?-\s*published)" -Message "workflow must trigger on release with types published"
  }

  It "grants contents write permission for release asset uploads" {
    $workflow = Get-Content -Path $script:workflowPath -Raw

    Assert-Match -Value $workflow -Pattern "permissions:\s*[\s\S]*?contents:\s*write" -Message "workflow must grant contents: write to upload the .deb to the release"
  }

  It "pins the job to ubuntu-24.04 and never ubuntu-latest" {
    $workflow = Get-Content -Path $script:workflowPath -Raw

    Assert-Match -Value $workflow -Pattern "runs-on:\s*ubuntu-24\.04" -Message "build job must pin ubuntu-24.04 for webkit2gtk-4.1 availability"
    Assert-True -Condition (-not ($workflow -match "ubuntu-latest")) -Message "workflow must not use ubuntu-latest; webkit2gtk-4.1 is pinned to ubuntu-24.04"
  }

  It "installs the required apt build dependencies" {
    $workflow = Get-Content -Path $script:workflowPath -Raw

    Assert-Match -Value $workflow -Pattern "apt-get\s+(-y\s+)?install|apt-get\s+install" -Message "workflow must install system dependencies via apt-get install"
    Assert-Match -Value $workflow -Pattern "libwebkit2gtk-4\.1-dev" -Message "apt step must install libwebkit2gtk-4.1-dev"
    Assert-Match -Value $workflow -Pattern "libgtk-3-dev" -Message "apt step must install libgtk-3-dev"
    Assert-Match -Value $workflow -Pattern "libayatana-appindicator3-dev" -Message "apt step must install libayatana-appindicator3-dev"
    Assert-Match -Value $workflow -Pattern "librsvg2-dev" -Message "apt step must install librsvg2-dev"
    Assert-Match -Value $workflow -Pattern "libdbus-1-dev" -Message "apt step must install libdbus-1-dev (keyring sync-secret-service build dependency)"
    Assert-Match -Value $workflow -Pattern "libxdo-dev" -Message "apt step must install libxdo-dev"
  }

  It "sets up pnpm before Node 22 and installs a stable Rust toolchain with cache" {
    $workflow = Get-Content -Path $script:workflowPath -Raw

    $pnpmIndex = $workflow.IndexOf("pnpm/action-setup")
    $nodeIndex = $workflow.IndexOf("actions/setup-node")

    Assert-True -Condition ($pnpmIndex -ge 0) -Message "workflow must use pnpm/action-setup"
    Assert-True -Condition ($nodeIndex -ge 0) -Message "workflow must use actions/setup-node"
    Assert-True -Condition ($pnpmIndex -lt $nodeIndex) -Message "pnpm/action-setup must run before actions/setup-node so node can resolve the pnpm cache"

    Assert-Match -Value $workflow -Pattern "actions/setup-node@v\d+[\s\S]*?node-version:\s*22" -Message "workflow must pin Node 22 (repo engines require >=22; node 20 jobs are legacy)"
    Assert-Match -Value $workflow -Pattern "dtolnay/rust-toolchain@stable" -Message "workflow must install a stable Rust toolchain via dtolnay/rust-toolchain@stable or equivalent"
    Assert-Match -Value $workflow -Pattern "Swatinem/rust-cache@v2" -Message "workflow must cache Rust builds with Swatinem/rust-cache@v2"
  }

  It "installs dependencies with a frozen lockfile and builds only the deb bundle" {
    $workflow = Get-Content -Path $script:workflowPath -Raw

    Assert-Match -Value $workflow -Pattern "pnpm\s+install\s+--frozen-lockfile" -Message "workflow must install dependencies with pnpm install --frozen-lockfile"
    Assert-Match -Value $workflow -Pattern "pnpm\s+--filter\s+@entropia/desktop\s+tauri\s+build\s+--bundles\s+deb" -Message "workflow must build with pnpm --filter @entropia/desktop tauri build --bundles deb"
  }

  It "smoke checks the produced .deb with dpkg-deb for pdfium and gstreamer" {
    $workflow = Get-Content -Path $script:workflowPath -Raw

    Assert-Match -Value $workflow -Pattern "dpkg-deb\s+(--info|-I)" -Message "smoke check must inspect package metadata via dpkg-deb --info"
    Assert-Match -Value $workflow -Pattern "dpkg-deb\s+(--contents|-c)" -Message "smoke check must list package contents via dpkg-deb --contents"
    Assert-Match -Value $workflow -Pattern "dpkg-deb[\s\S]*?libpdfium\.so" -Message "smoke check must verify the .deb ships libpdfium.so"
    Assert-Match -Value $workflow -Pattern "dpkg-deb[\s\S]*?gstreamer" -Message "smoke check must verify the Depends field includes gstreamer"
  }

  It "uploads the .deb as a workflow artifact" {
    $workflow = Get-Content -Path $script:workflowPath -Raw

    Assert-Match -Value $workflow -Pattern "actions/upload-artifact@v\d+[\s\S]*?path:\s*apps/desktop/src-tauri/target/release/bundle/deb/\*\.deb" -Message "workflow must upload the .deb artifact from apps/desktop/src-tauri/target/release/bundle/deb/*.deb"
  }

  It "uploads the .deb to the GitHub release only on release events" {
    $workflow = Get-Content -Path $script:workflowPath -Raw

    Assert-Match -Value $workflow -Pattern "if:\s*github\.event_name\s*==\s*'release'" -Message "release upload step must be conditional on github.event_name == 'release'"
    Assert-Match -Value $workflow -Pattern "if:\s*github\.event_name\s*==\s*'release'[\s\S]*?gh\s+release\s+upload" -Message "release upload step must use gh release upload"
    Assert-Match -Value $workflow -Pattern "GITHUB_TOKEN:\s*\$\{\{\s*secrets\.GITHUB_TOKEN\s*\}\}" -Message "release upload step must authenticate gh with secrets.GITHUB_TOKEN"
  }
}
