# ========================================
# Draveur Manager - Installation Windows
# ========================================

#Requires -RunAsAdministrator

$ErrorActionPreference = "Stop"

# Variables
$InstallDir = "$env:ProgramFiles\DraveurManager"
$DataDir = "$env:ProgramData\DraveurManager"
$RepoUrl = "https://github.com/thefrcrazy/draveur-manager"

# Couleurs
function Write-ColorOutput($ForegroundColor, $Message) {
    $fc = $host.UI.RawUI.ForegroundColor
    $host.UI.RawUI.ForegroundColor = $ForegroundColor
    Write-Output $Message
    $host.UI.RawUI.ForegroundColor = $fc
}

Write-Host ""
Write-ColorOutput Magenta "╔═══════════════════════════════════════════════════════════╗"
Write-ColorOutput Magenta "║              🎮 Draveur Manager Installer                 ║"
Write-ColorOutput Magenta "║           Gestionnaire de serveurs Hytale                 ║"
Write-ColorOutput Magenta "╚═══════════════════════════════════════════════════════════╝"
Write-Host ""

# Vérification des droits admin
$currentPrincipal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $currentPrincipal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-ColorOutput Red "❌ Ce script doit être exécuté en tant qu'Administrateur"
    exit 1
}

# Installation de Chocolatey si nécessaire
function Install-Chocolatey {
    if (Get-Command choco -ErrorAction SilentlyContinue) {
        Write-ColorOutput Green "✓ Chocolatey déjà installé"
    } else {
        Write-ColorOutput Cyan "📦 Installation de Chocolatey..."
        Set-ExecutionPolicy Bypass -Scope Process -Force
        [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager]::SecurityProtocol -bor 3072
        Invoke-Expression ((New-Object System.Net.WebClient).DownloadString('https://community.chocolatey.org/install.ps1'))
    }
}

# Installation des dépendances
function Install-Dependencies {
    Write-ColorOutput Cyan "📦 Installation des dépendances..."
    
    # Git
    if (Get-Command git -ErrorAction SilentlyContinue) {
        Write-ColorOutput Green "✓ Git déjà installé"
    } else {
        choco install git -y
    }
    
    # Rust
    if (Get-Command rustc -ErrorAction SilentlyContinue) {
        Write-ColorOutput Green "✓ Rust déjà installé"
    } else {
        Write-ColorOutput Cyan "🦀 Installation de Rust..."
        choco install rust -y
    }
    
    # Bun
    if (Get-Command bun -ErrorAction SilentlyContinue) {
        Write-ColorOutput Green "✓ Bun déjà installé"
    } else {
        Write-ColorOutput Cyan "🍞 Installation de Bun..."
        Invoke-RestMethod -Uri "https://bun.sh/install.ps1" | Invoke-Expression
    }
    
    # Refresh PATH
    $env:Path = [System.Environment]::GetEnvironmentVariable("Path","Machine") + ";" + [System.Environment]::GetEnvironmentVariable("Path","User")
}

# Installation de Java 25
function Install-Java {
    $javaVersion = (java -version 2>&1 | Select-String "version").ToString()
    if ($javaVersion -match "25") {
        Write-ColorOutput Green "✓ Java 25 déjà installé"
    } else {
        Write-ColorOutput Cyan "☕ Installation de Java 25 (Adoptium Temurin)..."
        choco install temurin25 -y
    }
}

# Clonage et compilation
function Build-Project {
    Write-ColorOutput Cyan "📥 Téléchargement du projet..."
    
    if (Test-Path $InstallDir) {
        Set-Location $InstallDir
        git pull
    } else {
        git clone $RepoUrl $InstallDir
        Set-Location $InstallDir
    }
    
    Write-ColorOutput Cyan "🔨 Compilation du backend Rust..."
    Set-Location backend
    cargo build --release
    
    Write-ColorOutput Cyan "🎨 Build du frontend..."
    Set-Location ..\frontend
    bun install
    bun run build
    
    # Copier le frontend
    New-Item -ItemType Directory -Force -Path ..\backend\static | Out-Null
    Copy-Item -Path dist\* -Destination ..\backend\static -Recurse -Force
}

# Configuration
function Setup-Config {
    Write-ColorOutput Cyan "⚙️ Configuration..."
    
    New-Item -ItemType Directory -Force -Path "$DataDir\servers" | Out-Null
    New-Item -ItemType Directory -Force -Path "$DataDir\backups" | Out-Null
    New-Item -ItemType Directory -Force -Path "$DataDir\data" | Out-Null
    
    $jwtSecret = -join ((65..90) + (97..122) + (48..57) | Get-Random -Count 32 | ForEach-Object {[char]$_})
    
    @"
HOST=0.0.0.0
PORT=5500
DATABASE_URL=sqlite:$DataDir\data\draveur.db?mode=rwc
JWT_SECRET=$jwtSecret
SERVERS_DIR=$DataDir\servers
BACKUPS_DIR=$DataDir\backups
RUST_LOG=info
"@ | Set-Content "$DataDir\.env"
}

# Création du service Windows
function Create-WindowsService {
    Write-ColorOutput Cyan "🔧 Configuration du service Windows..."
    
    $serviceName = "DraveurManager"
    $serviceExists = Get-Service -Name $serviceName -ErrorAction SilentlyContinue
    
    if ($serviceExists) {
        Stop-Service -Name $serviceName -Force -ErrorAction SilentlyContinue
        sc.exe delete $serviceName
    }
    
    # Utiliser NSSM pour créer le service
    if (-not (Get-Command nssm -ErrorAction SilentlyContinue)) {
        choco install nssm -y
        $env:Path = [System.Environment]::GetEnvironmentVariable("Path","Machine")
    }
    
    nssm install $serviceName "$InstallDir\backend\target\release\draveur.exe"
    nssm set $serviceName AppDirectory "$InstallDir\backend"
    nssm set $serviceName AppEnvironmentExtra "HOST=0.0.0.0" "PORT=5500" "DATABASE_URL=sqlite:$DataDir\data\draveur.db?mode=rwc"
    nssm set $serviceName DisplayName "Draveur Manager"
    nssm set $serviceName Description "Game Server Manager for Hytale"
    nssm set $serviceName Start SERVICE_AUTO_START
}

# Ouverture du pare-feu
function Configure-Firewall {
    Write-ColorOutput Cyan "🔥 Configuration du pare-feu..."
    
    $ruleName = "Draveur Manager"
    $ruleExists = Get-NetFirewallRule -DisplayName $ruleName -ErrorAction SilentlyContinue
    
    if (-not $ruleExists) {
        New-NetFirewallRule -DisplayName $ruleName -Direction Inbound -LocalPort 5500 -Protocol TCP -Action Allow
    }
}

# Affichage final
function Show-Success {
    $ip = (Get-NetIPAddress -AddressFamily IPv4 | Where-Object {$_.InterfaceAlias -notlike "*Loopback*"} | Select-Object -First 1).IPAddress
    
    Write-Host ""
    Write-ColorOutput Green "╔═══════════════════════════════════════════════════════════╗"
    Write-ColorOutput Green "║           ✅ Installation terminée avec succès !          ║"
    Write-ColorOutput Green "╚═══════════════════════════════════════════════════════════╝"
    Write-Host ""
    Write-Host "  📡 Interface web: http://${ip}:5500"
    Write-Host ""
    Write-ColorOutput Yellow "  Commandes utiles:"
    Write-Host "    Start-Service DraveurManager    # Démarrer"
    Write-Host "    Stop-Service DraveurManager     # Arrêter"
    Write-Host "    Get-Service DraveurManager      # Status"
    Write-Host ""
    Write-ColorOutput Magenta "  Premier démarrage:"
    Write-Host "    1. Démarrez le service: Start-Service DraveurManager"
    Write-Host "    2. Accédez à http://${ip}:5500"
    Write-Host "    3. Créez votre compte admin"
    Write-Host ""
}

# Main
try {
    Install-Chocolatey
    Install-Dependencies
    Install-Java
    Build-Project
    Setup-Config
    Create-WindowsService
    Configure-Firewall
    Show-Success
} catch {
    Write-ColorOutput Red "❌ Erreur: $_"
    exit 1
}
