# Installs Orthodox Navigator on Windows from the latest GitHub release, for the current user (no admin).
#
#     irm https://raw.githubusercontent.com/giixoo/orthodox-navigator/main/install.ps1 | iex
#
# Run it again to update. To remove:
#     & ([scriptblock]::Create((irm https://raw.githubusercontent.com/giixoo/orthodox-navigator/main/install.ps1))) -Uninstall
#
# It puts the program in %LOCALAPPDATA%\Programs\Orthodox Navigator (the same folder as the installer) and
# adds the line that loads the `on` command to your PowerShell profiles (Windows PowerShell 5 and pwsh 7).
# Settings: $env:ON_VERSION = '0.4.0' installs that version; $env:ON_INSTALL_DIR another folder;
# $env:ON_NO_PROFILE = '1' leaves the profiles alone.

param([switch] $Uninstall)

# in a script block, so that with `| iex` none of this changes the caller's session
& {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'   # Invoke-WebRequest is very slow with the progress bar in PowerShell 5
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $repo = 'giixoo/orthodox-navigator'
    $dir = if ($env:ON_INSTALL_DIR) { $env:ON_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\Orthodox Navigator' }
    $script = Join-Path $dir 'shell\on.ps1'
    $line = ". `"$script`""
    $docs = [Environment]::GetFolderPath('MyDocuments')
    $profiles = 'WindowsPowerShell', 'PowerShell' | ForEach-Object { Join-Path $docs "$_\Microsoft.PowerShell_profile.ps1" }

    if ($Uninstall) {
        foreach ($p in $profiles) {
            if (Test-Path -LiteralPath $p) {
                $old = @(Get-Content -LiteralPath $p)
                if ($old -contains $line) { Set-Content -LiteralPath $p -Value ($old | Where-Object { $_ -ne $line }) }
            }
        }
        if (Test-Path -LiteralPath $dir) { Remove-Item -LiteralPath $dir -Recurse -Force }
        Write-Host "Orthodox Navigator removed from $dir. Open a new terminal."
        return
    }

    if ($env:ON_VERSION) {
        $v = $env:ON_VERSION.TrimStart('v')
    } else {
        $v = (Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest" -UseBasicParsing).tag_name.TrimStart('v')
    }
    $name = "on-$v-windows-x64"
    $url = "https://github.com/$repo/releases/download/v$v/$name.zip"
    Write-Host "Orthodox Navigator $v -> $dir"

    $tmp = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName())
    New-Item -ItemType Directory $tmp | Out-Null
    try {
        Invoke-WebRequest $url -OutFile "$tmp\on.zip" -UseBasicParsing
        Expand-Archive "$tmp\on.zip" $tmp
        $src = Join-Path $tmp $name
        New-Item -ItemType Directory -Force "$dir\bin", "$dir\shell" | Out-Null
        # a running on.exe can't be overwritten, but it can be renamed
        Get-ChildItem "$dir\bin\on.old*.exe" -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
        if (Test-Path "$dir\bin\on.exe") {
            try { Remove-Item "$dir\bin\on.exe" -Force } catch { Rename-Item "$dir\bin\on.exe" "on.old.$(Get-Random).exe" }
        }
        Copy-Item "$src\bin\on.exe" "$dir\bin\"
        Copy-Item "$src\shell\on.ps1" "$dir\shell\" -Force
        Copy-Item "$src\README.md", "$src\LICENSE" $dir -Force
        Get-ChildItem $dir -Recurse -File | Unblock-File
    } finally {
        Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }

    if ($env:ON_NO_PROFILE -ne '1') {
        foreach ($p in $profiles) {
            $old = if (Test-Path -LiteralPath $p) { @(Get-Content -LiteralPath $p) } else { @() }
            # already loaded from somewhere (this install, the installer, a clone of the repository)
            if ($old | Where-Object { $_ -match 'on\.ps1' }) { continue }
            New-Item -ItemType Directory -Force (Split-Path $p) | Out-Null
            Add-Content -LiteralPath $p -Value $line
        }
        Write-Host 'Done. Open a new terminal and type: on'
    } else {
        Write-Host "Done. To get the on command, add this line to your `$PROFILE: $line"
    }
}
