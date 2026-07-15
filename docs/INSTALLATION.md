# Installation

## Compatibilité

La version 1.0 supporte exclusivement Linux AMD64, Windows AMD64 et les conteneurs Linux AMD64. L’archive Linux native est construite et testée sur Ubuntu 24.04 avec systemd et exige glibc 2.39 ou plus récent. Pour une autre distribution ou une glibc plus ancienne, utilisez l’image Docker épinglée par digest. ARM, Wine/Proton, conteneurs Windows et comptes Steam privés sont exclus.

## Linux natif

Téléchargez l’installateur et le checksum d’archive depuis les assets de la release, puis vérifiez séparément leurs bundles Sigstore produits par GitHub Actions. Ne lancez jamais directement un script distant via un pipe. `cosign` 3 est requis pour cette vérification initiale.

```bash
version=1.0.6
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
$Version = '1.0.6'
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

```bash
cd install/linux
export DMX_VERSION='1.0.6'
export DMX_IMAGE='ghcr.io/thefrcrazy/dmx-server-manager@sha256:<digest-du-manifeste-signé>'
sudo --preserve-env=DMX_VERSION,DMX_IMAGE ./bootstrap-docker.sh direct
docker compose pull
docker compose up -d
```

Le bootstrap doit être exécuté avec `sudo` : il crée la clé en `0400`, propriétaire `10001:10001`, sans lecture globale. Il rend aussi l’arborescence d’import lisible/traversable par le groupe `10001` avec des répertoires `0750` et des fichiers `0640`. Le conteneur principal s’exécute avec l’UID/GID `10001`, un système de fichiers racine en lecture seule, toutes les capabilities supprimées et `/data` dans un volume nommé. `network_mode: host` est intentionnel et obligatoire pour les ports dynamiques des jeux.

`cosign` 3 doit être installé sur l’hôte. Avant de créer ou modifier les fichiers locaux, le bootstrap vérifie la signature keyless de l’image avec l’issuer GitHub Actions et l’identité exacte `release.yml@refs/tags/v$DMX_VERSION`. Il refuse une version différente, une autre identité, un autre issuer, un tag ou un namespace différent. `DMX_IMAGE` doit être l’image officielle `@sha256:<digest>` indiquée par le manifeste de release signé. Le premier pull et toutes les mises à niveau utilisent donc exactement l’artefact authentifié.

Pour Traefik HTTPS, renseignez un DNS public pointant sur l’hôte, ouvrez TCP 80/443, puis :

```bash
DMX_DOMAIN=panel.example.com \
DMX_ACME_EMAIL=admin@example.com \
DMX_VERSION='1.0.6' \
DMX_IMAGE='ghcr.io/thefrcrazy/dmx-server-manager@sha256:<digest-du-manifeste-signé>' \
sudo --preserve-env=DMX_DOMAIN,DMX_ACME_EMAIL,DMX_VERSION,DMX_IMAGE ./bootstrap-docker.sh traefik
docker compose -f docker-compose.traefik.yml up -d
```

Le compose HTTPS épingle [Traefik `v3.7.7`](https://github.com/traefik/traefik/releases/tag/v3.7.7) par digest. Cette version corrige notamment les avis de sécurité publiés avec cette release; ne remplacez pas ce pin par `latest`.

## Docker Desktop Windows

Prérequis : Docker Desktop 4.34 ou plus récent, conteneurs Linux, réseau hôte activé et Enhanced Container Isolation désactivé. Utilisez un volume Docker/WSL pour les mondes actifs; un bind mount NTFS dégrade fortement les E/S et la sémantique des fichiers.

Le réseau hôte de Docker Desktop expose les ports via la VM Linux. Vérifiez chaque port de jeu depuis le LAN et Internet après installation.

## Premier Owner

La création initiale n’est permise que s’il n’existe aucun compte et si la requête vient de loopback, ou si un jeton d’installation temporaire a été configuré. Une installation native ouverte depuis `http://localhost:5500` n’a pas besoin de jeton.

Pour un premier accès distant derrière HTTPS, générez le jeton sur l’hôte sans le copier dans un historique de shell :

```bash
read -r DMX_SETUP_TOKEN < <(openssl rand -base64 32)
export DMX_SETUP_TOKEN
docker compose -f docker-compose.traefik.yml up -d --force-recreate panel
```

Saisissez cette valeur dans le champ demandé par l’écran de setup. Dès que l’Owner existe, retirez-la du conteneur :

```bash
unset DMX_SETUP_TOKEN
docker compose -f docker-compose.traefik.yml up -d --force-recreate panel
```

Si vous l’avez ajoutée à `install/linux/.env`, supprimez entièrement la ligne `DMX_SETUP_TOKEN` avant la recréation. Sur une installation native distante, placez temporairement `setup_token = "..."` dans le fichier de configuration protégé, puis supprimez la ligne et redémarrez le service. Créez immédiatement l’Owner et conservez les rôles à privilèges élevés au strict minimum. `DMX_SESSION_TTL_HOURS` (ou `session_ttl_hours`) règle la durée de session entre 1 et 720 heures ; la valeur par défaut est 24.
