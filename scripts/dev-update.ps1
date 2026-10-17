# Yap Dev: bring this checkout up to date with origin/main before it starts,
# so the "Yap - Dev" desktop shortcut always runs the latest merged changes.
# Called by dev.bat. Never fails the launch: anything unusual (another
# branch, real local edits, offline, a merge that isn't a fast-forward) is
# said and skipped, and Yap starts with the code already here.
# ASCII only: Windows PowerShell 5.1 reads a BOM-less script as ANSI.

$ErrorActionPreference = 'Continue'
# git prints UTF-8 (commit titles have dashes and arrows); read and show it so.
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
Set-Location (Split-Path -Parent $PSScriptRoot)

function Say([string]$Text, [string]$Colour = 'Gray') {
    Write-Host "  $Text" -ForegroundColor $Colour
}

try {
    $branch = (git branch --show-current) 2>$null
    if ($LASTEXITCODE -ne 0) { Say 'Not a git checkout: not updating.' Yellow; exit 0 }
    if ($branch -ne 'main') {
        Say "On branch '$branch', not main: not updating." Yellow
        exit 0
    }

    # The Tauri CLI rewrites src-tauri/Cargo.toml with other line endings, which
    # shows as a change and blocks the update. Put back any file whose only
    # difference is line endings; real edits stay.
    # (`git diff --name-only` doesn't list such a file: its content is the
    # same once normalised, but `git status` still has it modified.)
    foreach ($line in @(git -c core.quotepath=off status --porcelain --untracked-files=no)) {
        if ($line -notmatch '^ M (.+)$') { continue }
        $file = $Matches[1]
        git diff --ignore-cr-at-eol --quiet -- $file 2>$null
        if ($LASTEXITCODE -eq 0) { git checkout --quiet -- $file 2>$null }
    }
    git diff --ignore-cr-at-eol --quiet HEAD
    if ($LASTEXITCODE -ne 0) {
        Say 'This checkout has local edits, so it is not updated (running them as they are):' Yellow
        git status --short | Select-Object -First 10 | ForEach-Object { Say "  $_" Yellow }
        exit 0
    }

    $before = (git rev-parse HEAD)
    $lockBefore = (git rev-parse HEAD:package-lock.json)
    Say 'Checking GitHub for the latest version...'
    git fetch --quiet origin main
    if ($LASTEXITCODE -ne 0) { Say "Couldn't reach GitHub: running the version already here." Yellow; exit 0 }
    git merge --ff-only --quiet origin/main
    if ($LASTEXITCODE -ne 0) { Say "Couldn't move to the latest version: running the version already here." Yellow; exit 0 }

    $after = (git rev-parse HEAD)
    if ($before -eq $after) {
        Say 'Already up to date.' Green
    } else {
        Say 'Updated to the latest version. New since last time:' Green
        git log --oneline --no-merges "$before..$after" | Select-Object -First 12 | ForEach-Object { Say "  $_" }
    }

    # New or changed JavaScript dependencies: install them before Vite starts.
    $lockAfter = (git rev-parse HEAD:package-lock.json)
    if (($lockAfter -ne $lockBefore) -or -not (Test-Path 'node_modules')) {
        Say 'Installing updated dependencies (npm install)...'
        npm install --no-audit --no-fund
    }
    Say "Running $((git log -1 --format='%h %s') 2>$null)" Cyan
} catch {
    Say "Update skipped: $_" Yellow
}
exit 0
