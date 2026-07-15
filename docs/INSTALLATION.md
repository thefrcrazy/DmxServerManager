# Installation

## Compatibilité

La version 1.0 supporte exclusivement Linux AMD64, Windows AMD64 et les conteneurs Linux AMD64. L’archive Linux native est construite et testée sur Ubuntu 24.04 avec systemd et exige glibc 2.39 ou plus récent. Pour une autre distribution ou une glibc plus ancienne, utilisez l’image Docker épinglée par digest. ARM, Wine/Proton, conteneurs Windows et comptes Steam privés sont exclus.

## Linux natif

Téléchargez l’installateur et le checksum d’archive depuis les assets de la release, puis vérifiez séparément leurs bundles Sigstore produits par GitHub Actions. Ne lancez jamais directement un script distant via un pipe. `cosign` 3 est requis pour cette vérification initiale.

```bash
version=1.0.8
asset="dmx-server-manager-v${version}-x86_64-unknown-linux-gnu.tar.gz"
installer="dmx-server-manager-install-linux.sh"
base="https://github.com/thefrcrazy/DmxServerManager/releases/download/v${version}"
curl -fLO "$base/$asset.sha256"
curl -fLO "$base/$asset.sha256.sigstore.json"
curl -fLO "$base/$installer"
curl -fLO "$base/$installer.sigstore.json"
cosign verify-blob \
  --bundle "$asset.sha256.sigstore.json" \
  --certificate-identity "https://github.com/thefrcrazy/DmxServerManager/.github/workflows/release.yml@refs/tags/v${version}" \
  --certificate-oidc-issuer 'https://token.actions.githubusercontent.com' \
  "$asset.sha256"
cosign verify-blob \
  --bundle "$installer.sigstore.json" \
  --certificate-identity "https://github.com/thefrcrazy/DmxServerManager/.github/workflows/release.yml@refs/tags/v${version}" \
  --certificate-oidc-issuer 'https://token.actions.githubusercontent.com' \
  "$installer"
archive_sha256=$(awk 'NR == 1 { print $1 }' "$asset.sha256")
less "$installer"
sudo DMX_VERSION="$version" DMX_EXPECTED_ARCHIVE_SHA256="$archive_sha256" sh "$installer"
```

L’installateur refuse de démarrer sans `DMX_EXPECTED_ARCHIVE_SHA256`; il ne fait jamais confiance à un checksum téléchargé depuis le même origin que l’archive. Il exige aussi un frontend complet (`static/index.html` et `static/assets`). Il crée l’utilisateur système `dmx-server-manager`, installe la clé maître en `0640 root:dmx-server-manager`, bascule vers une release immuable identifiée par son digest puis valide `/api/v1/health`. Toute erreur restaure la version et l’état du service précédents. Il ne modifie pas le pare-feu.

Par défaut, il tente aussi d’installer `git` et `steamcmd` depuis les dépôts APT déjà configurés. Il n’ajoute pas de dépôt tiers : activez le composant officiel de votre distribution qui fournit SteamCMD (`multiverse` sur Ubuntu, `non-free` selon la version de Debian) avant l’installation. Une indisponibilité de ces paquets ne bloque pas le panneau, mais désactive explicitement Spigot pour Git, et Valheim/Palworld/Steam personnalisé pour SteamCMD. `DMX_INSTALL_GAME_TOOLCHAINS=0` désactive cette tentative; `DMX_STEAMCMD_PATH` permet de fournir un exécutable absolu déjà administré.

```bash
sudo systemctl status dmx-server-manager
sudo journalctl -u dmx-server-manager -f
```

Le panneau écoute par défaut sur `127.0.0.1:5500`; ouvrez-le via `http://localhost:5500`, dont le navigateur traite le loopback comme un contexte local. Configurez TLS ou un reverse proxy avant toute écoute distante.

## Windows natif

Dans PowerShell 5.1 ou 7 lancé en administrateur :

```powershell
$Version = '1.0.8'
$Asset = "dmx-server-manager-v$Version-x86_64-pc-windows-msvc.zip"
$Installer = 'dmx-server-manager-install-windows.ps1'
$Base = "https://github.com/thefrcrazy/DmxServerManager/releases/download/v$Version"
Invoke-WebRequest "$Base/$Asset.sha256" -OutFile "$Asset.sha256"
Invoke-WebRequest "$Base/$Asset.sha256.sigstore.json" -OutFile "$Asset.sha256.sigstore.json"
Invoke-WebRequest "$Base/$Installer" -OutFile $Installer
Invoke-WebRequest "$Base/$Installer.sigstore.json" -OutFile "$Installer.sigstore.json"
cosign verify-blob `
  --bundle "$Asset.sha256.sigstore.json" `
  --certificate-identity "https://github.com/thefrcrazy/DmxServerManager/.github/workflows/release.yml@refs/tags/v$Version" `
  --certificate-oidc-issuer 'https://token.actions.githubusercontent.com' `
  "$Asset.sha256"
cosign verify-blob `
  --bundle "$Installer.sigstore.json" `
  --certificate-identity "https://github.com/thefrcrazy/DmxServerManager/.github/workflows/release.yml@refs/tags/v$Version" `
  --certificate-oidc-issuer 'https://token.actions.githubusercontent.com' `
  $Installer
$ArchiveSha256 = ((Get-Content "$Asset.sha256" -Raw).Trim() -split '\s+')[0]
Get-Content $Installer
& ".\$Installer" -Version $Version -ExpectedArchiveSha256 $ArchiveSha256
```

Le service `DmxServerManager` utilise son compte virtuel `NT SERVICE\DmxServerManager`, pas `LocalSystem`. La configuration et la clé sont sous `%PROGRAMDATA%\DmxServerManager\config` et ne sont jamais modifiables par le service. Les données écrites par le service sont isolées sous `%PROGRAMDATA%\DmxServerManager\data`. Aucun port de pare-feu n’est ouvert automatiquement.

L’installateur place le bootstrap SteamCMD officiel sous `%PROGRAMDATA%\DmxServerManager\data\toolchains\steamcmd`, refuse une archive de structure inattendue et exige une signature Authenticode Valve valide avant de l’exposer au service. `-SkipSteamCmd` permet de différer cette étape. Spigot nécessite en plus Git for Windows installé pour toute la machine; l’installateur ajoute ses répertoires contrôlés au `PATH` du service lorsqu’il le détecte.

## Docker Linux

Docker Engine et Docker Compose v2 doivent déjà être installés. L’installation du panneau ne nécessite ni clone Git, ni Bun, ni Rust, ni Cosign sur le serveur :

```bash
curl -fsSLo /tmp/dmx-server-manager-install-docker.sh \
  https://github.com/thefrcrazy/DmxServerManager/releases/latest/download/dmx-server-manager-install-docker.sh \
  && sudo sh /tmp/dmx-server-manager-install-docker.sh
```

L’installateur crée par défaut :

- `/opt/dmx-server-manager/docker-compose.yml`, que vous pouvez modifier librement ;
- `/opt/dmx-server-manager/config/config.toml` et la clé `config/master.key` ;
- `/opt/dmx-server-manager/data/`, qui contient SQLite, instances, mondes et sauvegardes ;
- `/opt/dmx-server-manager/.env`, avec l’image et le jeton temporaire du premier setup.

Une relance de l’installateur conserve les fichiers `docker-compose.yml`, `.env` et `config/config.toml` existants. Vos réseaux, labels et réglages de reverse proxy personnalisés ne sont donc pas écrasés.

Le conteneur s’exécute avec l’UID/GID `10001`, un système de fichiers racine en lecture seule et toutes les capabilities supprimées. Les deux dossiers sont montés directement : `./config:/config:ro` et `./data:/data`. `network_mode: host` est intentionnel et obligatoire pour les ports dynamiques des jeux.

L’image par défaut est `ghcr.io/thefrcrazy/dmx-server-manager:latest`. Pour épingler une version ou un digest signé, modifiez `DMX_IMAGE` dans `.env`. Le tag versionné n’est jamais déplacé ; seul `latest` suit la dernière release publiée et validée.

### Reverse proxy externe

DmxServerManager ne déploie aucun Traefik, certificat ou conteneur proxy. Dans `config/config.toml`, conservez le loopback pour un accès local, ou configurez une adresse joignable uniquement par votre reverse proxy HTTPS :

```toml
bind = "127.0.0.1:5500"
reverse_proxy = true
trusted_proxies = ["IP_EXACTE_DU_PROXY"]
```

Si le proxy est dans un autre conteneur, adaptez votre propre réseau et le `bind` sans exposer le port 5500 en HTTP public. La liste accepte des IP exactes, pas un réseau entier.

### Migration depuis l’ancien Compose du dépôt

Arrêtez d’utiliser le dépôt sur le serveur, mais conservez-le pendant la migration. L’installateur peut arrêter l’ancienne stack, copier le volume nommé `dmx-server-manager-data` vers `data/` et reprendre exactement l’ancienne clé maître :

```bash
curl -fsSLo /tmp/dmx-server-manager-install-docker.sh \
  https://github.com/thefrcrazy/DmxServerManager/releases/latest/download/dmx-server-manager-install-docker.sh \
  && sudo DMX_LEGACY_DIR="$HOME/DmxServerManager/install/linux" \
    sh /tmp/dmx-server-manager-install-docker.sh
```

La source et l’ancien volume ne sont pas supprimés. L’installateur refuse de copier une base SQLite active, d’écraser un dossier `data/` non vide ou de migrer sans la clé maître de 32 octets.

## Docker Desktop Windows

Prérequis : Docker Desktop 4.34 ou plus récent, conteneurs Linux, réseau hôte activé et Enhanced Container Isolation désactivé. Utilisez un volume Docker/WSL pour les mondes actifs; un bind mount NTFS dégrade fortement les E/S et la sémantique des fichiers.

Le réseau hôte de Docker Desktop expose les ports via la VM Linux. Vérifiez chaque port de jeu depuis le LAN et Internet après installation.

## Premier Owner

La création initiale n’est permise que s’il n’existe aucun compte et si la requête vient de loopback, ou si un jeton d’installation temporaire a été configuré. Une installation native ouverte depuis `http://localhost:5500` n’a pas besoin de jeton.

L’installateur Docker crée automatiquement un jeton initial dans `config/setup-token` et l’injecte depuis `.env`. Affichez-le uniquement au moment de créer l’Owner :

```bash
sudo cat /opt/dmx-server-manager/config/setup-token
```

Saisissez cette valeur dans l’écran de setup. Dès que l’Owner existe, retirez la variable et le fichier, puis recréez le panneau :

```bash
cd /opt/dmx-server-manager
sed -i '/^DMX_SETUP_TOKEN=/d' .env
sudo rm -f config/setup-token
docker compose up -d --force-recreate panel
```

Sur une installation native distante, placez temporairement `setup_token = "..."` dans le fichier de configuration protégé, puis supprimez la ligne et redémarrez le service. Créez immédiatement l’Owner et conservez les rôles à privilèges élevés au strict minimum. `session_ttl_hours` règle la durée de session entre 1 et 720 heures ; la valeur par défaut est 24.
