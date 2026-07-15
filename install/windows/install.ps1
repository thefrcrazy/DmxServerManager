#Requires -RunAsAdministrator

[CmdletBinding()]
param(
    [ValidatePattern('^[0-9A-Za-z.+-]+$')]
    [string]$Version = $(if ($env:DMX_VERSION) { $env:DMX_VERSION } else { '1.0.2' }),

    [string]$ExpectedArchiveSha256 = $(if ($env:DMX_EXPECTED_ARCHIVE_SHA256) { $env:DMX_EXPECTED_ARCHIVE_SHA256 } else { '' }),

    [string]$ArchivePath = $(if ($env:DMX_ARCHIVE_FILE) { $env:DMX_ARCHIVE_FILE } else { '' }),

    [string]$InstallDir = $(if ($env:DMX_INSTALL_DIR) { $env:DMX_INSTALL_DIR } else { Join-Path $env:ProgramFiles 'DmxServerManager' }),

    [string]$DataDir = $(if ($env:DMX_DATA_DIR) { $env:DMX_DATA_DIR } else { Join-Path $env:ProgramData 'DmxServerManager' }),

    [switch]$SkipSteamCmd,

    [switch]$NoStart
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[System.Net.ServicePointManager]::SecurityProtocol = [System.Net.SecurityProtocolType]::Tls12

$ServiceName = 'DmxServerManager'
$Repository = if ($env:DMX_REPOSITORY) { $env:DMX_REPOSITORY } else { 'thefrcrazy/DmxServerManager' }
$Asset = "dmx-server-manager-v$Version-x86_64-pc-windows-msvc.zip"
$BaseUrl = if ($env:DMX_RELEASE_BASE_URL) {
    $env:DMX_RELEASE_BASE_URL.TrimEnd('/')
} else {
    "https://github.com/$Repository/releases/download/v$Version"
}
$HealthUrl = if ($env:DMX_HEALTH_URL) { $env:DMX_HEALTH_URL } else { 'http://127.0.0.1:5500/api/v1/health' }

function Resolve-DmxManagedPath {
    param(
        [Parameter(Mandatory)][string]$PathValue,
        [Parameter(Mandatory)][string]$Label
    )

    if ($PathValue -notmatch '^[A-Za-z]:[\\/]') {
        throw "$Label must be an absolute path on a local Windows drive."
    }
    if ($PathValue.Substring(2).Contains(':')) {
        throw "$Label must not contain an alternate data stream."
    }
    $fullPath = [System.IO.Path]::GetFullPath($PathValue)
    $root = [System.IO.Path]::GetPathRoot($fullPath)
    $normalized = $fullPath.TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    $normalizedRoot = $root.TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    if ($normalized.Equals($normalizedRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "$Label must not be a filesystem root."
    }
    return $normalized
}

function Test-DmxPathOverlap {
    param(
        [Parameter(Mandatory)][string]$First,
        [Parameter(Mandatory)][string]$Second
    )

    if ($First.Equals($Second, [System.StringComparison]::OrdinalIgnoreCase)) {
        return $true
    }
    $separator = [System.IO.Path]::DirectorySeparatorChar
    return $First.StartsWith("$Second$separator", [System.StringComparison]::OrdinalIgnoreCase) -or
        $Second.StartsWith("$First$separator", [System.StringComparison]::OrdinalIgnoreCase)
}

function Assert-DmxManagedDirectory {
    param(
        [Parameter(Mandatory)][string]$PathValue,
        [Parameter(Mandatory)][string]$Label
    )

    $item = Get-Item -LiteralPath $PathValue -Force -ErrorAction SilentlyContinue
    if ($item -and
        (-not $item.PSIsContainer -or ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint))) {
        throw "Refusing a linked or non-directory $Label path: $PathValue"
    }
}

$InstallDir = Resolve-DmxManagedPath -PathValue $InstallDir -Label 'InstallDir/DMX_INSTALL_DIR'
$DataDir = Resolve-DmxManagedPath -PathValue $DataDir -Label 'DataDir/DMX_DATA_DIR'
if (Test-DmxPathOverlap -First $InstallDir -Second $DataDir) {
    throw 'InstallDir/DMX_INSTALL_DIR and DataDir/DMX_DATA_DIR must be disjoint paths.'
}

$ConfigDir = Join-Path $DataDir 'config'
$RuntimeDataDir = Join-Path $DataDir 'data'
$ReleasesDir = Join-Path $InstallDir 'releases'
$CurrentDir = Join-Path $InstallDir 'current'
$SteamCmdPath = if ($env:DMX_STEAMCMD_PATH) {
    $env:DMX_STEAMCMD_PATH
} else {
    Join-Path $RuntimeDataDir 'toolchains\steamcmd\steamcmd.exe'
}
$SteamCmdUrl = 'https://steamcdn-a.akamaihd.net/client/installer/steamcmd.zip'

if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne [System.Runtime.InteropServices.Architecture]::X64) {
    throw 'DmxServerManager 1.0 supports Windows AMD64 only.'
}
if (-not $ExpectedArchiveSha256) {
    throw 'ExpectedArchiveSha256 is required; obtain it from the verified signed release checksum.'
}
if ($ExpectedArchiveSha256 -notmatch '^[A-Fa-f0-9]{64}$') {
    throw 'ExpectedArchiveSha256 must be an exact SHA-256 digest.'
}
if ($ArchivePath) {
    if ($ArchivePath -notmatch '^[A-Za-z]:[\\/]') {
        throw 'ArchivePath must be an absolute local Windows path.'
    }
    if (-not (Test-Path -LiteralPath $ArchivePath -PathType Leaf)) {
        throw "ArchivePath is not a regular file: $ArchivePath"
    }
    $ArchivePath = (Get-Item -LiteralPath $ArchivePath).FullName
}
if ($SteamCmdPath -notmatch '^[A-Za-z]:[\\/]') {
    throw 'DMX_STEAMCMD_PATH must be an absolute local Windows path.'
}

function Invoke-Sc {
    param([Parameter(Mandatory)][string[]]$Arguments)

    $output = & "$env:SystemRoot\System32\sc.exe" @Arguments 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "sc.exe $($Arguments -join ' ') failed: $output"
    }
}

function Set-RestrictedAcl {
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][string]$ServiceAccount,
        [Parameter(Mandatory)][ValidateSet('Read', 'Modify')][string]$Access
    )

    $serviceGrant = if ($Access -eq 'Modify') { "${ServiceAccount}:(OI)(CI)M" } else { "${ServiceAccount}:(OI)(CI)RX" }
    $aclArguments = @($Path, '/inheritance:r', '/grant:r', 'SYSTEM:(OI)(CI)F', 'BUILTIN\Administrators:(OI)(CI)F', $serviceGrant)
    & "$env:SystemRoot\System32\icacls.exe" @aclArguments | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "Unable to secure ACLs on $Path"
    }
}

function Set-AdminOnlyAcl {
    param([Parameter(Mandatory)][string]$Path)

    $aclArguments = @($Path, '/inheritance:r', '/grant:r', 'SYSTEM:(OI)(CI)F', 'BUILTIN\Administrators:(OI)(CI)F')
    & "$env:SystemRoot\System32\icacls.exe" @aclArguments | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "Unable to apply the initial administrator-only ACL on $Path"
    }
}

function ConvertTo-TomlLiteral {
    param([Parameter(Mandatory)][string]$Value)

    if ($Value.Contains("'") -or $Value.Contains("`r") -or $Value.Contains("`n")) {
        throw "Path cannot be represented safely in config.toml: $Value"
    }
    return "'$Value'"
}

function Wait-PanelHealth {
    for ($attempt = 0; $attempt -lt 45; $attempt++) {
        $service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
        if (-not $service -or $service.Status -eq 'Stopped') {
            return $false
        }
        try {
            $response = Invoke-WebRequest -UseBasicParsing -Uri $HealthUrl -TimeoutSec 3
            if ($response.StatusCode -ge 200 -and $response.StatusCode -lt 300) {
                return $true
            }
        } catch {
            Start-Sleep -Seconds 2
        }
    }
    return $false
}

function Assert-ValveSteamCmd {
    param([Parameter(Mandatory)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "SteamCMD executable is missing: $Path"
    }
    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    if ($signature.Status -ne [System.Management.Automation.SignatureStatus]::Valid -or
        -not $signature.SignerCertificate -or
        $signature.SignerCertificate.Subject -notmatch '(?i)Valve') {
        throw "SteamCMD does not carry a valid Valve Authenticode signature: $Path"
    }
}

function Install-SteamCmd {
    param(
        [Parameter(Mandatory)][string]$Destination,
        [Parameter(Mandatory)][string]$TemporaryRoot
    )

    if (Test-Path -LiteralPath $Destination -PathType Leaf) {
        Assert-ValveSteamCmd -Path $Destination
        return
    }
    $destinationDirectory = Split-Path -Parent $Destination
    if (Test-Path -LiteralPath $destinationDirectory) {
        $unexpected = Get-ChildItem -LiteralPath $destinationDirectory -Force | Select-Object -First 1
        if ($unexpected) {
            throw "Refusing to replace an incomplete non-empty SteamCMD directory: $destinationDirectory"
        }
    } else {
        New-Item -ItemType Directory -Path $destinationDirectory -Force | Out-Null
    }

    $steamArchive = Join-Path $TemporaryRoot 'steamcmd.zip'
    Write-Host 'Downloading the official SteamCMD bootstrap...'
    Invoke-WebRequest -UseBasicParsing -MaximumRedirection 0 -Uri $SteamCmdUrl -OutFile $steamArchive
    $archiveSize = (Get-Item -LiteralPath $steamArchive).Length
    if ($archiveSize -lt 1024 -or $archiveSize -gt 16MB) {
        throw "The SteamCMD bootstrap archive has an invalid size: $archiveSize bytes"
    }

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [System.IO.Compression.ZipFile]::OpenRead($steamArchive)
    $stagingDirectory = Join-Path $TemporaryRoot 'steamcmd-staging'
    New-Item -ItemType Directory -Path $stagingDirectory | Out-Null
    $stagedExecutable = Join-Path $stagingDirectory 'steamcmd.exe'
    try {
        if ($zip.Entries.Count -ne 1 -or $zip.Entries[0].FullName -cne 'steamcmd.exe') {
            throw 'The SteamCMD bootstrap archive does not have the expected single-file structure.'
        }
        $entry = $zip.Entries[0]
        if ($entry.Length -lt 1024 -or $entry.Length -gt 32MB) {
            throw "The SteamCMD executable has an invalid size: $($entry.Length) bytes"
        }
        $input = $entry.Open()
        $output = [System.IO.File]::Open(
            $stagedExecutable,
            [System.IO.FileMode]::CreateNew,
            [System.IO.FileAccess]::Write,
            [System.IO.FileShare]::None
        )
        try {
            $input.CopyTo($output)
            $output.Flush()
        } finally {
            $output.Dispose()
            $input.Dispose()
        }
    } finally {
        $zip.Dispose()
    }
    Assert-ValveSteamCmd -Path $stagedExecutable
    Move-Item -LiteralPath $stagedExecutable -Destination $Destination
    Write-Host "SteamCMD bootstrap installed at $Destination"
}

function Get-ServiceToolPath {
    $entries = @(
        (Join-Path $env:SystemRoot 'System32'),
        $env:SystemRoot,
        (Join-Path $env:SystemRoot 'System32\Wbem'),
        (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0')
    )
    $gitRoots = @((Join-Path $env:ProgramFiles 'Git'))
    if (${env:ProgramFiles(x86)}) {
        $gitRoots += (Join-Path ${env:ProgramFiles(x86)} 'Git')
    }
    $gitRoot = $gitRoots | Where-Object {
        (Test-Path -LiteralPath (Join-Path $_ 'cmd\git.exe') -PathType Leaf) -and
        (Test-Path -LiteralPath (Join-Path $_ 'bin\bash.exe') -PathType Leaf)
    } | Select-Object -First 1
    if ($gitRoot) {
        $entries += (Join-Path $gitRoot 'cmd')
        $entries += (Join-Path $gitRoot 'bin')
        $entries += (Join-Path $gitRoot 'usr\bin')
    } else {
        Write-Warning 'Git for Windows was not found. Install it machine-wide before using the Spigot BuildTools profile.'
    }
    return (($entries | Select-Object -Unique) -join ';')
}

$installMutex = [System.Threading.Mutex]::new($false, 'Global\DmxServerManagerInstaller')
$mutexAcquired = $installMutex.WaitOne(0)
if (-not $mutexAcquired) {
    $installMutex.Dispose()
    throw 'Another DmxServerManager installation is running.'
}

$tempDir = Join-Path ([System.IO.Path]::GetTempPath()) ("dmx-server-manager-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tempDir | Out-Null

try {
    $preflightService = Get-CimInstance Win32_Service -Filter "Name='$ServiceName'" -ErrorAction SilentlyContinue
    if ($preflightService) {
        $expectedServicePath = '"' + (Join-Path $CurrentDir 'dmx-server-manager.exe') + '" --service'
        $expectedServiceAccount = "NT SERVICE\$ServiceName"
        if (-not $preflightService.PathName.Equals($expectedServicePath, [System.StringComparison]::OrdinalIgnoreCase) -or
            -not $preflightService.StartName.Equals($expectedServiceAccount, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to replace an unmanaged Windows service named $ServiceName."
        }
    }

    $archivePath = Join-Path $tempDir $Asset

    if ($ArchivePath) {
        Write-Host "Installing DmxServerManager $Version from a local verified archive..."
        Copy-Item -LiteralPath $ArchivePath -Destination $archivePath
    } else {
        Write-Host "Downloading DmxServerManager $Version..."
        Invoke-WebRequest -UseBasicParsing -Uri "$BaseUrl/$Asset" -OutFile $archivePath
    }
    $expectedChecksum = $ExpectedArchiveSha256.ToLowerInvariant()
    $actualChecksum = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    if (-not $actualChecksum.Equals($expectedChecksum, [System.StringComparison]::Ordinal)) {
        throw 'Release checksum verification failed.'
    }

    $payloadDir = Join-Path $tempDir 'payload'
    New-Item -ItemType Directory -Path $payloadDir | Out-Null
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $payloadRoot = [System.IO.Path]::GetFullPath($payloadDir + [System.IO.Path]::DirectorySeparatorChar)
    $entryNames = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
    $zip = [System.IO.Compression.ZipFile]::OpenRead($archivePath)
    try {
        foreach ($entry in $zip.Entries) {
            $normalizedName = $entry.FullName.Replace('\', '/')
            $segments = $normalizedName.Split('/', [System.StringSplitOptions]::RemoveEmptyEntries)
            if ([System.IO.Path]::IsPathRooted($entry.FullName) -or $segments -contains '..' -or $normalizedName.Contains(':')) {
                throw "Release archive contains an unsafe path: $($entry.FullName)"
            }
            if (-not $entryNames.Add($normalizedName)) {
                throw "Release archive contains a duplicate path: $($entry.FullName)"
            }
            foreach ($segment in $segments) {
                if ($segment.EndsWith('.') -or $segment.EndsWith(' ') -or $segment -match '^(?i:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\..*)?$') {
                    throw "Release archive contains a Windows-reserved path: $($entry.FullName)"
                }
            }

            $destination = [System.IO.Path]::GetFullPath((Join-Path $payloadDir $entry.FullName))
            if (-not $destination.StartsWith($payloadRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
                throw "Release archive escapes its destination: $($entry.FullName)"
            }

            $unixFileType = ($entry.ExternalAttributes -shr 16) -band 0xF000
            if ($unixFileType -ne 0 -and $unixFileType -ne 0x4000 -and $unixFileType -ne 0x8000) {
                throw "Release archive contains a link or special file: $($entry.FullName)"
            }
        }
    } finally {
        $zip.Dispose()
    }
    Expand-Archive -LiteralPath $archivePath -DestinationPath $payloadDir

    $payloadBinary = Join-Path $payloadDir 'dmx-server-manager.exe'
    $staticSource = Join-Path $payloadDir 'static'
    $staticIndex = Join-Path $staticSource 'index.html'
    $staticAssets = Join-Path $staticSource 'assets'
    if (-not (Test-Path -LiteralPath $payloadBinary -PathType Leaf)) {
        throw 'The release archive does not contain dmx-server-manager.exe at its root.'
    }
    if (-not (Test-Path -LiteralPath $staticIndex -PathType Leaf)) {
        throw 'The release archive does not contain static/index.html.'
    }
    if (-not (Test-Path -LiteralPath $staticAssets -PathType Container) -or
        -not (Get-ChildItem -LiteralPath $staticAssets -File -Recurse | Select-Object -First 1)) {
        throw 'The release archive does not contain frontend assets.'
    }

    $dataDirExisted = Test-Path -LiteralPath $DataDir
    $configDirExisted = Test-Path -LiteralPath $ConfigDir
    $runtimeDataDirExisted = Test-Path -LiteralPath $RuntimeDataDir
    $serviceHomeDir = Join-Path $RuntimeDataDir 'home'
    $serviceTempDir = Join-Path $RuntimeDataDir 'tmp'
    Assert-DmxManagedDirectory -PathValue $InstallDir -Label 'installation'
    Assert-DmxManagedDirectory -PathValue $ReleasesDir -Label 'releases'
    Assert-DmxManagedDirectory -PathValue $DataDir -Label 'data'
    Assert-DmxManagedDirectory -PathValue $ConfigDir -Label 'configuration'
    Assert-DmxManagedDirectory -PathValue $RuntimeDataDir -Label 'runtime data'
    Assert-DmxManagedDirectory -PathValue $serviceHomeDir -Label 'service home'
    Assert-DmxManagedDirectory -PathValue $serviceTempDir -Label 'service temporary'
    New-Item -ItemType Directory -Force -Path $InstallDir, $ReleasesDir, $DataDir, $ConfigDir, $RuntimeDataDir, $serviceHomeDir, $serviceTempDir | Out-Null
    if (-not $dataDirExisted) { Set-AdminOnlyAcl -Path $DataDir }
    if (-not $configDirExisted) { Set-AdminOnlyAcl -Path $ConfigDir }
    if (-not $runtimeDataDirExisted) { Set-AdminOnlyAcl -Path $RuntimeDataDir }

    if ($SkipSteamCmd) {
        Write-Warning 'SteamCMD installation was skipped; Steam-backed profiles remain unavailable until DMX_STEAMCMD_PATH points to a valid Valve-signed executable.'
    } else {
        Install-SteamCmd -Destination $SteamCmdPath -TemporaryRoot $tempDir
    }
    $serviceToolPath = Get-ServiceToolPath

    $releaseId = "$Version-$actualChecksum"
    $releaseDir = Join-Path $ReleasesDir $releaseId
    if (Test-Path -LiteralPath $releaseDir) {
        $releaseMarker = Join-Path $releaseDir '.archive.sha256'
        if (-not (Test-Path -LiteralPath $releaseMarker -PathType Leaf) -or
            (Get-Content -LiteralPath $releaseMarker -Raw).Trim() -ne $actualChecksum -or
            -not (Test-Path -LiteralPath (Join-Path $releaseDir 'dmx-server-manager.exe') -PathType Leaf) -or
            -not (Test-Path -LiteralPath (Join-Path $releaseDir 'static\index.html') -PathType Leaf)) {
            throw "Existing immutable release is incomplete or has a different digest: $releaseDir"
        }
    } else {
        $stagingDir = Join-Path $ReleasesDir ('.staging-' + [guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $stagingDir | Out-Null
        try {
            Copy-Item -LiteralPath $payloadBinary -Destination (Join-Path $stagingDir 'dmx-server-manager.exe')
            Copy-Item -LiteralPath $staticSource -Destination $stagingDir -Recurse
            [System.IO.File]::WriteAllText(
                (Join-Path $stagingDir '.archive.sha256'),
                "$actualChecksum`n",
                [System.Text.UTF8Encoding]::new($false)
            )
            Move-Item -LiteralPath $stagingDir -Destination $releaseDir
        } finally {
            if (Test-Path -LiteralPath $stagingDir) {
                Remove-Item -LiteralPath $stagingDir -Recurse -Force
            }
        }
    }

    $configPath = Join-Path $ConfigDir 'config.toml'
    $masterKeyPath = Join-Path $ConfigDir 'master.key'
    $databasePath = (Join-Path $RuntimeDataDir 'dmx-server-manager.sqlite').Replace('\', '/')
    $stableStaticPath = Join-Path $CurrentDir 'static'

    $masterKeyItem = Get-Item -LiteralPath $masterKeyPath -Force -ErrorAction SilentlyContinue
    if ($masterKeyItem -and
        ($masterKeyItem.PSIsContainer -or ($masterKeyItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint))) {
        throw "Refusing a linked or non-regular master key: $masterKeyPath"
    }
    if (-not $masterKeyItem) {
        $masterKey = New-Object byte[] 32
        $random = [System.Security.Cryptography.RandomNumberGenerator]::Create()
        try {
            $random.GetBytes($masterKey)
            [System.IO.File]::WriteAllBytes($masterKeyPath, $masterKey)
        } finally {
            $random.Dispose()
            [Array]::Clear($masterKey, 0, $masterKey.Length)
        }
    }
    if ((Get-Item -LiteralPath $masterKeyPath -Force).Length -ne 32) {
        throw 'The master key must contain exactly 32 bytes.'
    }

    $configItem = Get-Item -LiteralPath $configPath -Force -ErrorAction SilentlyContinue
    if ($configItem -and
        ($configItem.PSIsContainer -or ($configItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint))) {
        throw "Refusing a linked or non-regular configuration file: $configPath"
    }
    if (-not $configItem) {
        $config = @(
            'bind = "127.0.0.1:5500"',
            "data_dir = $(ConvertTo-TomlLiteral $RuntimeDataDir)",
            "database_url = $(ConvertTo-TomlLiteral "sqlite://${databasePath}?mode=rwc")",
            "master_key_file = $(ConvertTo-TomlLiteral $masterKeyPath)",
            "steamcmd_path = $(ConvertTo-TomlLiteral $SteamCmdPath)",
            "static_dir = $(ConvertTo-TomlLiteral $stableStaticPath)",
            'reverse_proxy = false',
            'trusted_proxies = []',
            'import_roots = []',
            'log = "info"',
            '# Official release URL and Ed25519 public key are compiled into the binary.',
            '# Override release_manifest_url and release_public_key together only.'
        ) -join "`r`n"
        [System.IO.File]::WriteAllText($configPath, "$config`r`n", [System.Text.UTF8Encoding]::new($false))
    }

    $existingService = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
    $previousService = if ($existingService) { Get-CimInstance Win32_Service -Filter "Name='$ServiceName'" } else { $null }
    if ($previousService) {
        $expectedServicePath = '"' + (Join-Path $CurrentDir 'dmx-server-manager.exe') + '" --service'
        $expectedServiceAccount = "NT SERVICE\$ServiceName"
        if (-not $previousService.PathName.Equals($expectedServicePath, [System.StringComparison]::OrdinalIgnoreCase) -or
            -not $previousService.StartName.Equals($expectedServiceAccount, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to replace an unmanaged Windows service named $ServiceName."
        }
    }
    $serviceWasRunning = [bool]($existingService -and $existingService.Status -ne 'Stopped')
    $serviceCreated = $false
    $currentBackup = $null
    $currentCreated = $false
    $serviceRegistryPath = "HKLM:\SYSTEM\CurrentControlSet\Services\$ServiceName"
    $previousEnvironmentPresent = $false
    $previousEnvironment = $null
    $previousDelayedStartPresent = $false
    $previousDelayedStart = 0
    if ($existingService) {
        $environmentProperty = Get-ItemProperty -LiteralPath $serviceRegistryPath -Name Environment -ErrorAction SilentlyContinue
        if ($environmentProperty) {
            $previousEnvironmentPresent = $true
            $previousEnvironment = [string[]]$environmentProperty.Environment
        }
        $delayedStartProperty = Get-ItemProperty -LiteralPath $serviceRegistryPath -Name DelayedAutoStart -ErrorAction SilentlyContinue
        if ($delayedStartProperty) {
            $previousDelayedStartPresent = $true
            $previousDelayedStart = [int]$delayedStartProperty.DelayedAutoStart
        }
    }

    if (Test-Path -LiteralPath $CurrentDir) {
        $currentItem = Get-Item -LiteralPath $CurrentDir -Force
        if (-not ($currentItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint)) {
            throw "$CurrentDir exists but is not a junction or symbolic link."
        }
    }

    try {
        if ($serviceWasRunning) {
            Stop-Service -Name $ServiceName -Force
            $existingService.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(120))
        }

        if (Test-Path -LiteralPath $CurrentDir) {
            $currentBackup = Join-Path $InstallDir ('current.rollback-' + [guid]::NewGuid().ToString('N'))
            Move-Item -LiteralPath $CurrentDir -Destination $currentBackup
        }
        New-Item -ItemType Junction -Path $CurrentDir -Target $releaseDir | Out-Null
        $currentCreated = $true

        $binary = Join-Path $CurrentDir 'dmx-server-manager.exe'
        $binaryPath = "`"$binary`" --service"
        if ($existingService) {
            Invoke-Sc -Arguments @('config', $ServiceName, 'binPath=', $binaryPath, 'start=', 'auto')
        } else {
            Invoke-Sc -Arguments @('create', $ServiceName, 'binPath=', $binaryPath, 'start=', 'auto', 'DisplayName=', 'DmxServerManager')
            $serviceCreated = $true
        }

        $serviceAccount = "NT SERVICE\$ServiceName"
        Invoke-Sc -Arguments @('config', $ServiceName, 'obj=', $serviceAccount, 'password=', '')
        if ($serviceCreated) {
            Invoke-Sc -Arguments @('description', $ServiceName, 'DmxServerManager game server manager')
            Invoke-Sc -Arguments @('sidtype', $ServiceName, 'unrestricted')
            Invoke-Sc -Arguments @('failure', $ServiceName, 'reset= 86400', 'actions= restart/5000/restart/15000/none/0')
        }

        Set-RestrictedAcl -Path $InstallDir -ServiceAccount $serviceAccount -Access Read
        Set-RestrictedAcl -Path $DataDir -ServiceAccount $serviceAccount -Access Read
        Set-RestrictedAcl -Path $ConfigDir -ServiceAccount $serviceAccount -Access Read
        Set-RestrictedAcl -Path $RuntimeDataDir -ServiceAccount $serviceAccount -Access Modify
        $secretAclArguments = @($masterKeyPath, '/inheritance:r', '/grant:r', 'SYSTEM:F', 'BUILTIN\Administrators:F', "${serviceAccount}:R")
        & "$env:SystemRoot\System32\icacls.exe" @secretAclArguments | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw 'Unable to secure the master key ACL.'
        }

        $serviceEnvironment = @(
            "DMX_CONFIG_FILE=$configPath",
            "DMX_DATA_DIR=$RuntimeDataDir",
            "DMX_DATABASE_URL=sqlite://${databasePath}?mode=rwc",
            "DMX_MASTER_KEY_FILE=$masterKeyPath",
            "DMX_STEAMCMD_PATH=$SteamCmdPath",
            "DMX_STATIC_DIR=$stableStaticPath",
            "HOME=$serviceHomeDir",
            "USERPROFILE=$serviceHomeDir",
            "TEMP=$serviceTempDir",
            "TMP=$serviceTempDir",
            "PATH=$serviceToolPath",
            'DMX_DEPLOYMENT_MODE=native',
            'DMX_SERVICE_MODE=windows'
        )
        New-ItemProperty -Path $serviceRegistryPath -Name Environment -PropertyType MultiString -Value $serviceEnvironment -Force | Out-Null
        New-ItemProperty -Path $serviceRegistryPath -Name DelayedAutoStart -PropertyType DWord -Value 1 -Force | Out-Null

        Start-Service -Name $ServiceName
        (Get-Service -Name $ServiceName).WaitForStatus('Running', [TimeSpan]::FromSeconds(90))
        if (-not (Wait-PanelHealth)) {
            throw "Service did not pass its HTTP health check at $HealthUrl"
        }

        if ($NoStart) {
            Stop-Service -Name $ServiceName
            (Get-Service -Name $ServiceName).WaitForStatus('Stopped', [TimeSpan]::FromSeconds(120))
        }

        if ($currentBackup -and (Test-Path -LiteralPath $currentBackup)) {
            Remove-Item -LiteralPath $currentBackup -Force
            $currentBackup = $null
        }
    } catch {
        $installationError = $_
        Write-Warning 'Installation failed; restoring the previous Windows service state.'
        Stop-Service -Name $ServiceName -Force -ErrorAction SilentlyContinue

        try {
            if ($currentCreated -and (Test-Path -LiteralPath $CurrentDir)) {
                Remove-Item -LiteralPath $CurrentDir -Force
            }
            if ($currentBackup -and (Test-Path -LiteralPath $currentBackup)) {
                Move-Item -LiteralPath $currentBackup -Destination $CurrentDir
                $currentBackup = $null
            }
        } catch {
            Write-Warning "Unable to restore the previous current junction: $_"
        }

        if ($previousService) {
            $startMode = switch ($previousService.StartMode) {
                'Auto' { 'auto' }
                'Manual' { 'demand' }
                'Disabled' { 'disabled' }
                default { 'demand' }
            }
            try {
                Invoke-Sc -Arguments @('config', $ServiceName, 'binPath=', $previousService.PathName, 'start=', $startMode)
                if ($previousService.StartName) {
                    Invoke-Sc -Arguments @('config', $ServiceName, 'obj=', $previousService.StartName, 'password=', '')
                }
            } catch {
                Write-Warning "Unable to restore the previous SCM configuration: $_"
            }
            try {
                if ($previousEnvironmentPresent) {
                    New-ItemProperty -Path $serviceRegistryPath -Name Environment -PropertyType MultiString -Value $previousEnvironment -Force | Out-Null
                } else {
                    Remove-ItemProperty -LiteralPath $serviceRegistryPath -Name Environment -ErrorAction SilentlyContinue
                }
                if ($previousDelayedStartPresent) {
                    New-ItemProperty -Path $serviceRegistryPath -Name DelayedAutoStart -PropertyType DWord -Value $previousDelayedStart -Force | Out-Null
                } else {
                    Remove-ItemProperty -LiteralPath $serviceRegistryPath -Name DelayedAutoStart -ErrorAction SilentlyContinue
                }
            } catch {
                Write-Warning "Unable to restore the previous service environment: $_"
            }
            if ($serviceWasRunning) {
                Start-Service -Name $ServiceName -ErrorAction SilentlyContinue
            }
        } elseif ($serviceCreated) {
            try {
                Invoke-Sc -Arguments @('delete', $ServiceName)
            } catch {
                Write-Warning "Unable to remove the failed new service: $_"
            }
        }
        throw $installationError
    }

    Write-Host "DmxServerManager $Version is installed from immutable release $releaseId."
    if ($NoStart) {
        Write-Host 'The release passed its HTTP health check and was then stopped (-NoStart).'
    } else {
        Write-Host 'Open http://localhost:5500 locally to create the first Owner.'
    }
    Write-Host 'No firewall rule was created. Remote access requires TLS or a declared reverse proxy.'
} finally {
    if (Test-Path -LiteralPath $tempDir) {
        Remove-Item -LiteralPath $tempDir -Recurse -Force
    }
    if ($mutexAcquired) {
        $installMutex.ReleaseMutex()
    }
    $installMutex.Dispose()
}
