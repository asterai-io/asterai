#!/usr/bin/env pwsh

$ErrorActionPreference = "Stop"

$Repo = "asterai-io/asterai"
$GitHub = if ($env:GITHUB) { $env:GITHUB } else { "https://github.com" }
$BinDir = "${Home}\.local\bin"

# Check architecture
if (-not ((Get-CimInstance Win32_ComputerSystem).SystemType -match "x64-based")) {
  Write-Output "Install failed: asterai for Windows is currently only available for x86 64-bit."
  return 1
}

$Target = "win32-x64"
$Asset = "asterai-${Target}.zip"

if ($args.Count -gt 0) {
  $Tag = $args[0]
  $DownloadURL = "${GitHub}/${Repo}/releases/download/${Tag}/${Asset}"
} else {
  try {
    # Try latest release first, fall back to older releases if assets are still building.
    $DownloadURL = $null
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/${Repo}/releases/latest"
    $match = $release.assets | Where-Object { $_.name -eq $Asset }
    if ($match) {
      $Tag = $release.tag_name
      $DownloadURL = $match.browser_download_url
    }
    if (-not $DownloadURL) {
      $releases = Invoke-RestMethod -Uri "https://api.github.com/repos/${Repo}/releases"
      foreach ($r in $releases) {
        $match = $r.assets | Where-Object { $_.name -eq $Asset }
        if ($match) {
          $Tag = $r.tag_name
          $DownloadURL = $match.browser_download_url
          break
        }
      }
    }
    if (-not $DownloadURL) {
      Write-Output "Install failed: could not find a release with ${Asset}"
      return 1
    }
  } catch {
    Write-Output "Install failed: could not fetch releases from GitHub"
    return 1
  }
}

# Create bin directory
if (!(Test-Path $BinDir)) {
  New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
}

$TmpDir = Join-Path ([System.IO.Path]::GetTempPath()) "asterai-install"
if (Test-Path $TmpDir) { Remove-Item $TmpDir -Recurse -Force }
New-Item -ItemType Directory -Force -Path $TmpDir | Out-Null
$ZipPath = "${TmpDir}\${Asset}"

# Download
try {
  curl.exe "-#SfLo" "$ZipPath" "$DownloadURL"
  if ($LASTEXITCODE -ne 0) {
    throw "curl failed"
  }
} catch {
  try {
    Invoke-RestMethod -Uri $DownloadURL -OutFile $ZipPath
  } catch {
    Write-Output "Install failed: could not download asterai from ${DownloadURL}"
    return 1
  }
}

# Extract
try {
  Expand-Archive "$ZipPath" "$TmpDir" -Force
} catch {
  Write-Output "Install failed: could not extract ${Asset}"
  return 1
}

# Find and install binary
if (Test-Path "${TmpDir}\asterai.exe") {
  Copy-Item "${TmpDir}\asterai.exe" "${BinDir}\asterai.exe" -Force
} elseif (Test-Path "${TmpDir}\asterai\asterai.exe") {
  Copy-Item "${TmpDir}\asterai\asterai.exe" "${BinDir}\asterai.exe" -Force
} else {
  Write-Output "Install failed: asterai.exe not found after extraction."
  Remove-Item $TmpDir -Recurse -Force -ErrorAction SilentlyContinue
  return 1
}

# Clean up
Remove-Item $TmpDir -Recurse -Force -ErrorAction SilentlyContinue

# Add ~/.local/bin to PATH if needed, prepending so it takes priority
$UserPath = [System.Environment]::GetEnvironmentVariable("Path", "User") -split ';'
$NeedRestart = $false
if ($UserPath -notcontains $BinDir) {
  # Prepend so the native binary takes priority over npm shims etc.
  $UserPath = @($BinDir) + $UserPath
  [System.Environment]::SetEnvironmentVariable("Path", ($UserPath -join ';'), "User")
  $env:PATH = "${BinDir};${env:PATH}"
  $NeedRestart = $true
} else {
  # Already in PATH - ensure it's at the front so it takes priority
  $Idx = [Array]::IndexOf($UserPath, $BinDir)
  if ($Idx -gt 0) {
    $UserPath = @($BinDir) + ($UserPath | Where-Object { $_ -ne $BinDir })
    [System.Environment]::SetEnvironmentVariable("Path", ($UserPath -join ';'), "User")
    $env:PATH = "${BinDir};$($env:PATH -replace [regex]::Escape("${BinDir};"), '' -replace [regex]::Escape(";${BinDir}"), '')"
    $NeedRestart = $true
  }
}

if ($NeedRestart) {
  # Broadcast WM_SETTINGCHANGE so other processes pick up the new PATH
  $HWND_BROADCAST = [IntPtr] 0xffff
  $WM_SETTINGCHANGE = 0x1a
  $result = [UIntPtr]::Zero
  if (-not ("Win32.NativeMethods" -as [Type])) {
    Add-Type -Namespace Win32 -Name NativeMethods -MemberDefinition @"
[DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)]
public static extern IntPtr SendMessageTimeout(
    IntPtr hWnd, uint Msg, UIntPtr wParam, string lParam,
    uint fuFlags, uint uTimeout, out UIntPtr lpdwResult);
"@
  }
  [Win32.NativeMethods]::SendMessageTimeout($HWND_BROADCAST, $WM_SETTINGCHANGE, [UIntPtr]::Zero, "Environment", 2, 5000, [ref] $result) | Out-Null
}

# Show success
Write-Output ""
Write-Host "  ✓" -ForegroundColor Green -NoNewline
Write-Output " asterai ${Tag} installed to ${BinDir}"
Write-Output ""
Write-Output "  Run 'asterai --help' to get started with the CLI."
Write-Output "  Run 'asterai agents' to manage AI agents."
Write-Output ""

if ($NeedRestart) {
  Write-Output "  Restart your terminal for the PATH changes to take effect."
  Write-Output ""
}
