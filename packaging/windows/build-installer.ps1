<#
.SYNOPSIS
    Baut MQTT Probe und erzeugt den Windows-Installer (Inno Setup) in einem Lauf.

.DESCRIPTION
    Fuehrt alle Schritte aus der Build-Anleitung aus:
      1. Umgebung pruefen (cargo, rustc, ISCC) und CARGO_TARGET_DIR setzen
      2. Release-Build erzeugen
      3. EXE in den vom Installer erwarteten Pfad kopieren
      4. Installer mit ISCC kompilieren
      5. Ergebnis pruefen (Version, Groesse, Pruefsumme)

    WICHTIG: Das Target-Verzeichnis wird auf ein lokales NTFS-Laufwerk gelegt
    (Standard C:\cargo-target\mqtt-probe). Auf dem VirtualBox-Share X: scheitert
    der Linker mit LNK1104, weil rustc dort keine Hardlinks anlegen kann.

.PARAMETER SkipBuild
    Schritte 2 und 3 ueberspringen und den vorhandenen Release-Build verwenden.

.PARAMETER Clean
    Vor dem Build "cargo clean" ausfuehren. Achtung: kompiliert alle
    Abhaengigkeiten neu und dauert deutlich laenger.

.PARAMETER Test
    Nach dem Build "cargo test --release" ausfuehren. Schlaegt ein Test fehl,
    bricht das Skript ab.

.PARAMETER SkipVerify
    Ueberspringt die Version-Bindung (Pruefung, ob die Setup-Version der
    Version aus Cargo.toml entspricht).

.PARAMETER Ci
    Nicht-interaktiver Modus: nur Log-Ausgabe, keine Rueckfragen.

.EXAMPLE
    .\build-installer.ps1

.EXAMPLE
    .\build-installer.ps1 -Test

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File .\build-installer.ps1 -Ci
#>
[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [switch]$Clean,
    [switch]$Test,
    [switch]$SkipVerify,
    [switch]$Ci
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------------------
# Konfiguration (alle absoluten Pfade)
# ---------------------------------------------------------------------------
$RepoRoot       = 'X:\rust\mqtt-probe'
$CargoTargetDir = 'C:\cargo-target\mqtt-probe'
$IssPath        = 'X:\rust\mqtt-probe\packaging\windows\mqtt-probe.iss'
$CargoToml      = 'X:\rust\mqtt-probe\Cargo.toml'
$DistDir        = 'X:\rust\mqtt-probe\dist'
$StagedExe      = 'X:\rust\mqtt-probe\target\release\mqtt-probe.exe'
$IsccExe        = 'C:\Users\klumw\Apps\Inno Setup 6\ISCC.exe'
$IconPath       = 'X:\rust\mqtt-probe\assets\mqtt-probe.ico'
$LicensePath    = 'X:\rust\mqtt-probe\LICENSE'

# ---------------------------------------------------------------------------
# Hilfsfunktionen
# ---------------------------------------------------------------------------
$script:StepNo    = 0
$script:StepTotal = 5
$script:Stopwatch = [System.Diagnostics.Stopwatch]::StartNew()

function Write-Step {
    param([string]$Message)
    $script:StepNo++
    Write-Host ''
    Write-Host ("[{0}/{1}] {2}" -f $script:StepNo, $script:StepTotal, $Message) -ForegroundColor Cyan
    Write-Host ('-' * 72) -ForegroundColor DarkGray
}

function Write-Detail {
    param([string]$Message)
    Write-Host "      $Message" -ForegroundColor Gray
}

function Write-Ok {
    param([string]$Message)
    Write-Host "  [OK] $Message" -ForegroundColor Green
}

function Write-Warn {
    param([string]$Message)
    Write-Host "  [WARN] $Message" -ForegroundColor Yellow
}

function Stop-WithError {
    param([string]$Message, [string]$Hint)
    Write-Host ''
    Write-Host "  [FEHLER] $Message" -ForegroundColor Red
    if ($Hint) {
        Write-Host "  Hinweis : $Hint" -ForegroundColor Yellow
    }
    Write-Host ''
    exit 1
}

<#
    Fuehrt ein externes Kommando aus und liefert den Exit-Code.
    Die Ausgabe wird live durchgereicht; stderr-Warnungen erscheinen in
    dunkelgelb, normale Ausgabe in dunkelgrau.
#>
function Invoke-Tool {
    param(
        [Parameter(Mandatory = $true)][string]$Exe,
        [string[]]$Arguments = @(),
        [string]$WorkingDirectory,
        [string]$Caption
    )

    if ($Caption) {
        Write-Detail $Caption
    }

    $previous = $null
    if ($WorkingDirectory) {
        $previous = (Get-Location).Path
        Set-Location -LiteralPath $WorkingDirectory
    }

    # WICHTIG: Native Tools schreiben Warnungen (z. B. "could not canonicalize
    # path X:\..." auf dem VirtualBox-Share) auf stderr. PowerShell wandelt
    # stderr-Zeilen um, und bei $ErrorActionPreference = 'Stop' wird daraus ein
    # terminierender Fehler - obwohl der Build erfolgreich ist. Deshalb wird
    # $ErrorActionPreference hier auf Continue gesetzt; der Erfolg wird
    # ausschliesslich ueber $LASTEXITCODE bewertet.
    $previousEap = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        & $Exe @Arguments 2>&1 | ForEach-Object {
            if ($_ -is [System.Management.Automation.ErrorRecord]) {
                Write-Host ("      " + $_.ToString()) -ForegroundColor DarkYellow
            }
            else {
                Write-Host ("      " + $_) -ForegroundColor DarkGray
            }
        }
        return $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousEap
        if ($previous) {
            Set-Location -LiteralPath $previous
        }
    }
}

<#
    Liest die erste Ausgabezeile eines Tools ein, ohne dass stderr-Warnungen
    (z. B. "could not canonicalize path X:\...") bei
    $ErrorActionPreference = 'Stop' als terminierender Fehler behandelt werden.
#>
function Get-ToolVersionLine {
    param(
        [Parameter(Mandatory = $true)][string]$Exe,
        [string]$VersionArgument = '--version'
    )

    $previousEap = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $lines = & $Exe $VersionArgument 2>&1
        foreach ($line in $lines) {
            $text = $line.ToString()
            if ($text -match '^\s*(cargo|rustc)\s+\d+\.\d+') {
                return $text.Trim()
            }
        }
        return $null
    }
    finally {
        $ErrorActionPreference = $previousEap
    }
}

<#
    Liest die Version aus dem [package]-Abschnitt der Cargo.toml.
    Eigener Parser statt TOML-Bibliothek, damit keine Abhaengigkeit entsteht.
    Die Version wird nur VOR der naechsten [Sektion] gesucht, damit weder
    "vers" noch "rust-version" oder Dependency-Versionen faelschlich greifen.
#>
function Get-CargoPackageVersion {
    param([Parameter(Mandatory = $true)][string]$Path)

    $inPackage = $false
    foreach ($line in Get-Content -LiteralPath $Path) {
        $trimmed = $line.Trim()
        if ($trimmed.StartsWith('[')) {
            $inPackage = ($trimmed -eq '[package]')
            continue
        }
        if ($inPackage -and $trimmed -match '^version\s*=\s*"([^"]+)"') {
            return $Matches[1]
        }
    }
    return $null
}

# ---------------------------------------------------------------------------
# Vorspann
# ---------------------------------------------------------------------------
Write-Host ''
Write-Host '====================================================================' -ForegroundColor White
Write-Host ' MQTT Probe - Build und Installer' -ForegroundColor White
Write-Host '====================================================================' -ForegroundColor White
Write-Detail ("PowerShell  : {0}" -f $PSVersionTable.PSVersion.ToString())
Write-Detail ("Repository  : {0}" -f $RepoRoot)
Write-Detail ("Cargo-Target: {0}" -f $CargoTargetDir)

# ---------------------------------------------------------------------------
# Schritt 1: Umgebung pruefen
# ---------------------------------------------------------------------------
Write-Step 'Umgebung pruefen'

if (-not (Test-Path -LiteralPath $RepoRoot -PathType Container)) {
    Stop-WithError "Repository nicht gefunden: $RepoRoot" 'Laufwerk X: verbunden? VirtualBox-Share gemountet?'
}
if (-not (Test-Path -LiteralPath $IssPath -PathType Leaf)) {
    Stop-WithError "Inno-Setup-Skript nicht gefunden: $IssPath" 'Pfad im Abschnitt "Konfiguration" des Skripts anpassen.'
}
if (-not (Test-Path -LiteralPath $CargoToml -PathType Leaf)) {
    Stop-WithError "Cargo.toml nicht gefunden: $CargoToml"
}
foreach ($required in @($IconPath, $LicensePath)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
        Stop-WithError "Vom Installer benoetigte Datei fehlt: $required"
    }
}
Write-Ok 'Repository, .iss und Quelldateien vorhanden'

$cargoCmd = Get-Command cargo -ErrorAction SilentlyContinue
if (-not $cargoCmd) {
    Stop-WithError 'cargo nicht gefunden (nicht im PATH).' 'Rust installieren oder %USERPROFILE%\.cargo\bin zum PATH hinzufuegen.'
}
$rustcCmd = Get-Command rustc -ErrorAction SilentlyContinue
if (-not $rustcCmd) {
    Stop-WithError 'rustc nicht gefunden (nicht im PATH).' 'Rust-Toolchain mit rustup installieren.'
}
$cargoVersion = Get-ToolVersionLine -Exe $cargoCmd.Source
$rustcVersion = Get-ToolVersionLine -Exe $rustcCmd.Source
if (-not $cargoVersion) { Stop-WithError 'cargo --version liefert keine verwertbare Ausgabe.' }
if (-not $rustcVersion) { Stop-WithError 'rustc --version liefert keine verwertbare Ausgabe.' }
Write-Detail ("cargo: {0}" -f $cargoVersion)
Write-Detail ("rustc: {0}" -f $rustcVersion)

if ($rustcVersion -match 'rustc (\d+)\.(\d+)\.(\d+)') {
    $major = [int]$Matches[1]
    $minor = [int]$Matches[2]
    if ($major -lt 1 -or ($major -eq 1 -and $minor -lt 95)) {
        Stop-WithError ("rustc {0}.{1} ist zu alt, benoetigt wird 1.95+" -f $major, $minor) 'rustup update stable'
    }
    Write-Ok ("rustc {0}.{1}.{2} erfuellt die Mindestversion 1.95" -f $major, $minor, [int]$Matches[3])
}

if (-not (Test-Path -LiteralPath $IsccExe -PathType Leaf)) {
    Stop-WithError "Inno Setup Compiler nicht gefunden: $IsccExe" 'Inno Setup 6 installieren oder $IsccExe im Abschnitt "Konfiguration" anpassen.'
}
Write-Ok 'Inno Setup Compiler (ISCC) gefunden'

# Netzwerkpfade fuer das Target sind nicht zulaessig (LNK1104).
$targetRoot = [System.IO.Path]::GetPathRoot($CargoTargetDir)
if ($targetRoot.StartsWith('\\')) {
    Stop-WithError "Cargo-Target liegt auf einem Netzwerkpfad: $CargoTargetDir" 'Auf ein lokales NTFS-Laufwerk umstellen, z. B. C:\cargo-target\mqtt-probe'
}
if (-not (Test-Path -LiteralPath $targetRoot)) {
    Stop-WithError "Laufwerk fuer Cargo-Target nicht verfuegbar: $targetRoot"
}
Write-Ok ("Cargo-Target auf lokalem Laufwerk: {0}" -f $targetRoot)

$env:CARGO_TARGET_DIR = $CargoTargetDir
Write-Detail ("CARGO_TARGET_DIR = {0}" -f $env:CARGO_TARGET_DIR)

$appVersion = Get-CargoPackageVersion -Path $CargoToml
if (-not $appVersion) {
    Stop-WithError "Version im [package]-Abschnitt von $CargoToml nicht lesbar."
}
Write-Ok ("App-Version laut Cargo.toml: {0}" -f $appVersion)

# ---------------------------------------------------------------------------
# Schritt 2: Release-Build
# ---------------------------------------------------------------------------
Write-Step 'Release-Build erzeugen'

if ($SkipBuild) {
    Write-Warn 'Uebersprungen (-SkipBuild)'
}
else {
    if ($Clean) {
        Write-Warn 'cargo clean: alle Abhaengigkeiten werden neu kompiliert (dauert deutlich laenger)'
        $cleanExit = Invoke-Tool -Exe $cargoCmd.Source -Arguments @('clean') -WorkingDirectory $RepoRoot
        if ($cleanExit -ne 0) {
            Stop-WithError "cargo clean fehlgeschlagen (Exit-Code $cleanExit)"
        }
    }

    $buildStart = Get-Date
    $buildExit = Invoke-Tool -Exe $cargoCmd.Source -Arguments @('build', '--release') -WorkingDirectory $RepoRoot -Caption 'cargo build --release'
    $buildSpan = (Get-Date) - $buildStart

    if ($buildExit -ne 0) {
        Stop-WithError "cargo build fehlgeschlagen (Exit-Code $buildExit)" 'Die Build-Ausgabe oberhalb nennt den Compilerfehler.'
    }
    Write-Ok ("cargo build --release erfolgreich in {0:hh\:mm\:ss}" -f $buildSpan)
}

# ---------------------------------------------------------------------------
# Schritt 3: EXE in den Installer-Pfad kopieren
# ---------------------------------------------------------------------------
Write-Step 'EXE in den Installer-Pfad kopieren'

$builtExe = Join-Path $CargoTargetDir 'release\mqtt-probe.exe'
if (-not (Test-Path -LiteralPath $builtExe -PathType Leaf)) {
    Stop-WithError "Gebaute EXE fehlt: $builtExe" 'Schritt 2 nicht ueberspringen oder "-SkipBuild" entfernen.'
}
if ($SkipBuild) {
    Write-Warn 'Uebersprungen (-SkipBuild): vorhandene EXE wird verwendet'
}
else {
    $stagedDir = Split-Path -Parent $StagedExe
    if (-not (Test-Path -LiteralPath $stagedDir -PathType Container)) {
        $null = New-Item -ItemType Directory -Path $stagedDir -Force
    }
    Copy-Item -LiteralPath $builtExe -Destination $StagedExe -Force
}
$stagedInfo = Get-Item -LiteralPath $StagedExe
$builtHash  = (Get-FileHash -LiteralPath $builtExe  -Algorithm SHA256).Hash
$stagedHash = (Get-FileHash -LiteralPath $StagedExe -Algorithm SHA256).Hash
if ($builtHash -ne $stagedHash) {
    Stop-WithError "Kopie weicht von der gebauten EXE ab: $StagedExe" ("Gebaut: {0} / Kopiert: {1}" -f $builtHash, $stagedHash)
}
Write-Ok ("{0} ({1:N0} Bytes, {2:yyyy-MM-dd HH:mm:ss})" -f $StagedExe, $stagedInfo.Length, $stagedInfo.LastWriteTime)

if ($Test -and -not $SkipBuild) {
    $testExit = Invoke-Tool -Exe $cargoCmd.Source -Arguments @('test', '--release') -WorkingDirectory $RepoRoot -Caption 'cargo test --release'
    if ($testExit -ne 0) {
        Stop-WithError "cargo test fehlgeschlagen (Exit-Code $testExit)"
    }
    Write-Ok 'Tests erfolgreich'
}

# ---------------------------------------------------------------------------
# Schritt 4: Installer kompilieren
# ---------------------------------------------------------------------------
Write-Step 'Installer kompilieren (Inno Setup)'

# Bewusst KEIN /O-Schalter: ISCC bricht damit grundsaetzlich mit
# "I/O error 123" ab, weil er mit der OutputDir-Direktive kollidiert.
# Der Ausgabeort kommt ausschliesslich aus der .iss.
$isccExit = Invoke-Tool -Exe $IsccExe -Arguments @($IssPath) -Caption ("ISCC.exe {0}" -f $IssPath)
if ($isccExit -ne 0) {
    Stop-WithError "ISCC fehlgeschlagen (Exit-Code $isccExit)" 'Haeufigste Ursachen: relative Pfade in der .iss (I/O error 123), gesperrte Zieldatei oder Inno noch geoeffnet.'
}
Write-Ok 'Installer kompiliert'

# ---------------------------------------------------------------------------
# Schritt 5: Ergebnis pruefen
# ---------------------------------------------------------------------------
Write-Step 'Ergebnis pruefen'

$expectedSetup = Join-Path $DistDir ("MQTT-Probe-{0}-setup.exe" -f $appVersion)
if (-not (Test-Path -LiteralPath $expectedSetup -PathType Leaf)) {
    $found = @(Get-ChildItem -LiteralPath $DistDir -Filter 'MQTT-Probe-*-setup.exe' -ErrorAction SilentlyContinue)
    if ($found.Count -eq 0) {
        Stop-WithError "Kein Installer in $DistDir gefunden" 'OutputDir in der .iss pruefen.'
    }
    $names = ($found | ForEach-Object { $_.Name }) -join ', '
    Stop-WithError ("Installer mit erwarteter Version fehlt: {0}" -f $expectedSetup) ("Gefunden: {0} - deutet auf abweichende Version in Cargo.toml/ReadIni hin." -f $names)
}

$setupInfo        = Get-Item -LiteralPath $expectedSetup
$setupVersionInfo = $setupInfo.VersionInfo
# VersionInfo liefert Werte mit Leerzeichen-Padding ("0.8.1              ").
$setupFileVersion = ($setupVersionInfo.FileVersion -as [string])
if ($setupFileVersion) { $setupFileVersion = $setupFileVersion.Trim() }

Write-Detail ("Datei        : {0}" -f $setupInfo.FullName)
Write-Detail ("Groesse      : {0:N0} Bytes" -f $setupInfo.Length)
Write-Detail ("Erstellt     : {0:yyyy-MM-dd HH:mm:ss}" -f $setupInfo.LastWriteTime)
Write-Detail ("FileVersion  : {0}" -f $setupFileVersion)
Write-Detail ("Beschreibung : {0}" -f $setupVersionInfo.FileDescription)
Write-Detail ("Firma        : {0}" -f $setupVersionInfo.CompanyName)
Write-Detail ("SHA-256      : {0}" -f (Get-FileHash -LiteralPath $expectedSetup -Algorithm SHA256).Hash)

$stale = @(Get-ChildItem -LiteralPath $DistDir -Filter 'MQTT-Probe-*-setup.exe' | Where-Object { $_.FullName -ne $setupInfo.FullName })
if ($stale.Count -gt 0) {
    Write-Warn ("Weitere Setup-Dateien in dist (aeltere Builds): {0}" -f (($stale | ForEach-Object { $_.Name }) -join ', '))
}

if (-not $SkipVerify) {
    if ($setupFileVersion -ne $appVersion) {
        Stop-WithError ("Version im Installer ({0}) weicht von Cargo.toml ({1}) ab." -f $setupFileVersion, $appVersion) 'MyAppVersion in der .iss pruefen: ReadIni(Cargo.toml, "package", "version").'
    }
    Write-Ok ("Version im Installer stimmt mit Cargo.toml ueberein: {0}" -f $appVersion)
}

$strayRoot = @(Get-ChildItem -LiteralPath $RepoRoot -Filter 'MQTT-Probe-*-setup.exe' -File -ErrorAction SilentlyContinue)
if ($strayRoot.Count -gt 0) {
    Write-Warn ("Setup-Datei(en) im Repository-Wurzelverzeichnis: {0}" -f (($strayRoot | ForEach-Object { $_.Name }) -join ', '))
}

# ---------------------------------------------------------------------------
# Zusammenfassung
# ---------------------------------------------------------------------------
$script:Stopwatch.Stop()
Write-Host ''
Write-Host '====================================================================' -ForegroundColor White
Write-Host ' FERTIG' -ForegroundColor Green
Write-Host '====================================================================' -ForegroundColor White
Write-Detail ("EXE      : {0}" -f $StagedExe)
Write-Detail ("Installer: {0}" -f $setupInfo.FullName)
Write-Detail ("Dauer    : {0:hh\:mm\:ss}" -f $script:Stopwatch.Elapsed)
Write-Host ''
exit 0
