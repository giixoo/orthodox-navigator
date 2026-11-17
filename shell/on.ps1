# Orthodox Navigator shell integration for PowerShell.
#
# Add this line to your $PROFILE (run `notepad $PROFILE`):
#     . C:\work\my\projects\orthodox-navigator\shell\on.ps1
#
# Then type `on`. Commands you type in the file manager run right here, in this PowerShell session -
# your aliases, functions, modules and variables work, and a `cd` is followed. The file manager steps
# aside while a command runs and comes back exactly as it was (Ctrl+O shows the output).
# When you quit (Alt+X) you stay in the folder of the active panel, and every command you ran is
# in your history (Up arrow, Ctrl+R, Get-History).
#
# Set $env:ORTHODOX_NAVIGATOR_EXE to start a different build.

$global:OrthodoxNavigatorExe = Join-Path $PSScriptRoot '..\bin\on.exe'

function global:Add-OrthodoxNavigatorHistory([string] $File) {
    foreach ($line in [IO.File]::ReadAllLines($File)) {
        if (-not $line.Trim()) { continue }
        # Up arrow / Ctrl+R (PSReadLine)
        try { [Microsoft.PowerShell.PSConsoleReadLine]::AddToHistory($line) } catch { }
        # Get-History / Invoke-History
        $now = Get-Date
        Add-History -InputObject ([pscustomobject]@{
            CommandLine        = $line
            ExecutionStatus    = [System.Management.Automation.Runspaces.PipelineState]::Completed
            StartExecutionTime = $now
            EndExecutionTime   = $now.AddMilliseconds(1)
        })
    }
    [IO.File]::WriteAllText($File, '')
}

function global:Invoke-OrthodoxNavigator {
    param([string] $Exe, [string[]] $Arguments)
    if (-not (Test-Path -LiteralPath $Exe)) {
        Write-Error "Orthodox Navigator not found at $Exe. Build it first (see README.md)."
        return
    }

    $__on = @{
        Cwd     = [IO.Path]::GetTempFileName()
        History = [IO.Path]::GetTempFileName()
        Session = [IO.Path]::GetTempFileName()
        Action  = [IO.Path]::GetTempFileName()
        Shell   = (Get-Process -Id $PID).Path   # this same PowerShell (pwsh or Windows PowerShell)
    }
    $__onSkip = @('_', '?', '^', '$', 'args', 'input', 'PSItem', 'this', 'Matches', 'LASTEXITCODE', 'PSCmdlet',
        'MyInvocation', 'PSBoundParameters', 'foreach', 'switch', 'PSScriptRoot', 'PSCommandPath', 'StackTrace', 'Error')
    try {
        while ($true) {
            [IO.File]::WriteAllText($__on.Action, '')
            & $Exe --cwd-file $__on.Cwd --history-file $__on.History --shell $__on.Shell `
                --session-file $__on.Session --action-file $__on.Action @Arguments

            Add-OrthodoxNavigatorHistory $__on.History
            $__onDir = [IO.File]::ReadAllText($__on.Cwd).Trim()
            if ($__onDir -and (Test-Path -LiteralPath $__onDir)) { Set-Location -LiteralPath $__onDir }

            $__onAction = [IO.File]::ReadAllText($__on.Action)
            if (-not $__onAction.StartsWith("run`n")) { break }

            # run the command here, in this session, as if typed at the prompt
            $__onCommand = $__onAction.Substring(4)
            Write-Host "PS $PWD> $__onCommand"
            $__onBefore = @(Get-Variable -Scope 0 | ForEach-Object Name)
            try { . ([scriptblock]::Create($__onCommand)) }
            catch { Write-Host $_ -ForegroundColor Red }
            # keep variables the command created (this function runs in its own scope)
            foreach ($__onVar in @(Get-Variable -Scope 0)) {
                if ($__onBefore -notcontains $__onVar.Name -and $__onSkip -notcontains $__onVar.Name -and -not $__onVar.Name.StartsWith('__on')) {
                    Set-Variable -Name $__onVar.Name -Value $__onVar.Value -Scope Global
                }
            }
            Write-Host ''
        }
    }
    finally {
        Remove-Item -LiteralPath $__on.Cwd, $__on.History, $__on.Session, $__on.Action -ErrorAction SilentlyContinue
    }
}

function global:on {
    $exe = if ($env:ORTHODOX_NAVIGATOR_EXE) { $env:ORTHODOX_NAVIGATOR_EXE } else { $global:OrthodoxNavigatorExe }
    Invoke-OrthodoxNavigator -Exe $exe -Arguments $args
}
