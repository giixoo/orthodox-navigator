# Adds (or with -Remove takes out) the line that loads the `on` shell function to the PowerShell
# profiles of the current user: Windows PowerShell 5 and PowerShell 7 (pwsh). Run by the installer.
param([Parameter(Mandatory)] [string] $Script, [switch] $Remove)

$docs = [Environment]::GetFolderPath('MyDocuments')
$line = ". `"$Script`""
foreach ($dir in 'WindowsPowerShell', 'PowerShell') {
    $profilePath = Join-Path $docs "$dir\Microsoft.PowerShell_profile.ps1"
    $old = if (Test-Path -LiteralPath $profilePath) { @(Get-Content -LiteralPath $profilePath) } else { @() }
    if ($Remove) {
        if ($old -contains $line) {
            Set-Content -LiteralPath $profilePath -Value ($old | Where-Object { $_ -ne $line })
        }
        continue
    }
    # already loaded from somewhere (a clone of the repository, say)
    if ($old | Where-Object { $_ -match 'on\.ps1' }) { continue }
    New-Item -ItemType Directory -Force (Split-Path $profilePath) | Out-Null
    Add-Content -LiteralPath $profilePath -Value $line
}
