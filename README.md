# DmxServerManager

[![CI](https://github.com/thefrcrazy/DmxServerManager/actions/workflows/ci.yml/badge.svg)](https://github.com/thefrcrazy/DmxServerManager/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Gestionnaire mono-hôte de serveurs de jeux, écrit en Rust et React. La version 1.0 cible Linux AMD64 natif sur Ubuntu 24.04/glibc 2.39+, Windows AMD64 et les conteneurs Linux AMD64. Elle utilise SQLite, des comptes locaux et des sauvegardes locales.

> [!IMPORTANT]
> Pour une installation de production, utilisez exclusivement une release signée et ses checksums. Une archive construite directement depuis une branche Git n’est pas un artefact de publication.

## Fonctionnalités

- installation et mise à jour transactionnelles avec staging, validation et rollback ;
- supervision des groupes de processus, console, watchdog, métriques et journaux en temps réel par SSE ;
- file de jobs persistante pour les installations, mises à jour, sauvegardes, restaurations et imports ;
- sessions opaques, CSRF, RBAC, affectation des instances et journal d’audit ;
- sauvegardes locales vérifiées, gestionnaire de fichiers cloisonné et imports ZIP sécurisés ;
- tâches planifiées, notifications, chat d’équipe, webhooks Discord et catalogue local `.dmxpack` ;
- interface React responsive en français et en anglais, pilotée par les capacités de chaque profil, avec sélecteur visuel et illustrations locales des jeux.

## Profils 1.0

| Profil | Distribution | Ports par défaut | Particularités |
|---|---|---|---|
| Hytale | downloader officiel, Java 25 géré | UDP 5520 | OAuth device par instance et mise à jour atomique |
| Minecraft Java | Vanilla, Paper, Fabric, Forge, NeoForge, Spigot, Purpur et Quilt | TCP 25565 | versions et loaders détectés auprès des fournisseurs officiels, EULA explicite |
| Minecraft Bedrock | archive officielle Linux ou Windows découverte automatiquement | UDP 19132 et 19133 | mondes, allowlist, permissions et packs |
| Valheim | SteamCMD anonyme, AppID `896660` | UDP `N` et `N+1` | Crossplay optionnel et sauvegardes isolées |
| Palworld | SteamCMD anonyme, AppID `2394010` | UDP 8211 | paramètres INI sûrs, REST/RCON désactivés par défaut |
| Steam personnalisé | dépôt anonyme natif | déclarés par le profil | AppID numérique, exécutable relatif, aucun shell |

Les binaires de jeux ne sont jamais inclus dans l’image ou les releases. Les installateurs officiels et SteamCMD les téléchargent à la demande, selon leurs licences.

Pendant une installation ou une mise à jour, la page de l’instance bascule sur l’onglet Terminal et affiche les sorties de l’installateur en temps réel. L’historique borné est relu depuis le disque après une navigation, un rechargement ou une reconnexion SSE ; les invites sans saut de ligne sont également affichées. Lorsqu’une action humaine est nécessaire, le job passe à `waiting_for_user` et conserve une action explicite dans la page Jobs et sur l’instance. Pour Hytale, le code expiré est remplacé automatiquement dans la carte dès que le downloader en émet un nouveau, et le bouton ouvre l’URL officielle complète liée exactement au code affiché. Les installateurs n’acceptent jamais de saisie shell arbitraire.

La création d’une instance Minecraft utilise des sélecteurs alimentés côté serveur : manifest Mojang, Fill Paper, Fabric Meta, Maven Forge/NeoForge, API Purpur et Quilt Meta. Minecraft Bedrock essaie les deux endpoints officiels du catalogue Microsoft pour découvrir l’archive stable correspondant à l’OS, valide strictement l’URL officielle, calcule le SHA-256 téléchargé puis contrôle le ZIP avant activation.

Un AppID Steam personnalisé n’est pas automatiquement compatible. Le dépôt doit autoriser la connexion anonyme, fournir un exécutable natif AMD64 pour l’OS hôte et réussir la validation du profil. Les comptes Steam privés, Wine et Proton ne sont pas pris en charge en 1.0.

Les builds officiels surveillent par défaut un manifeste de release Ed25519 avec checksums complets, à partir d’une URL et d’une clé publique intégrées au binaire. Le panneau affiche une procédure native ou une image Docker épinglée par digest uniquement pour une version strictement plus récente ; il n’exécute jamais la mise à niveau et ne se remplace pas lui-même.

## Architecture du dépôt

| Répertoire | Contenu |
|---|---|
| `backend/` | API Axum `/api/v1`, SQLite/SQLx, profils, jobs, sécurité et supervision |
| `frontend/` | SPA React 19, Vite 8, TypeScript 6, client API généré depuis OpenAPI |
| `install/` | image Docker, Compose, service systemd et installateur Windows |
| `docs/` | installation, configuration, sécurité, profils et exploitation |
| `.github/workflows/` | CI, artefacts signés, SBOM, image GHCR et publication |

## Installation rapide avec Docker

Docker Engine, Docker Compose v2 et Linux AMD64 sont requis. Aucun clone Git n’est nécessaire.

### Docker Compose

Créez `/opt/dmx-server-manager/docker-compose.yml` avec ce contenu :

```yaml
name: dmx-server-manager

services:
  panel:
    image: "${DMX_IMAGE:-ghcr.io/thefrcrazy/dmx-server-manager:latest}"
    container_name: dmx-server-manager
    platform: linux/amd64
    restart: unless-stopped
    network_mode: host
    user: "10001:10001"
    read_only: true
    cap_drop:
      - ALL
    security_opt:
      - no-new-privileges:true
    stop_grace_period: 2m
    pids_limit: 4096
    environment:
      TZ: ${DMX_TIMEZONE:-Etc/UTC}
      DMX_CONFIG_FILE: /config/config.toml
      DMX_SETUP_TOKEN: ${DMX_SETUP_TOKEN:-}
    volumes:
      - ./config:/config:ro
      - ./data:/data
    tmpfs:
      - /tmp:size=256m,mode=1777
      - /run:size=16m,mode=0755
    healthcheck:
      test: ["CMD", "curl", "--fail", "--silent", "--show-error", "http://127.0.0.1:5500/api/v1/health"]
      interval: 30s
      timeout: 5s
      retries: 5
      start_period: 30s
```

Le même fichier signé est téléchargeable depuis chaque release. Télécharger uniquement le Compose ne suffit pas : `config/config.toml`, `config/master.key`, `.env` et `data/` doivent être créés avant le premier démarrage.

La variable `DMX_CONTAINER_UID` ci-dessous doit correspondre à la ligne `user:` du Compose :

- Compose officiel `user: "10001:10001"` : conservez `DMX_CONTAINER_UID=10001` ;
- Compose personnalisé `user: "1001:1001"` : utilisez `DMX_CONTAINER_UID=1001`.

Pour une installation neuve :

```bash
sudo install -d -m 0750 -o "$(id -u)" -g "$(id -g)" /opt/dmx-server-manager
cd /opt/dmx-server-manager
docker compose down 2>/dev/null || true

mkdir -p config data

curl -fsSLo docker-compose.yml \
  https://github.com/thefrcrazy/DmxServerManager/releases/latest/download/docker-compose.yml
curl -fsSLo config/config.toml \
  https://github.com/thefrcrazy/DmxServerManager/releases/latest/download/config.docker.example.toml

umask 077
openssl rand 32 > config/master.key
setup_token=$(openssl rand -base64 32 | tr -d '\n')
printf 'DMX_IMAGE=ghcr.io/thefrcrazy/dmx-server-manager:latest\nDMX_TIMEZONE=Europe/Paris\nDMX_SETUP_TOKEN=%s\n' \
  "$setup_token" > .env
unset setup_token

DMX_CONTAINER_UID=10001
# Si votre Compose contient user: "1001:1001", utilisez plutôt :
# DMX_CONTAINER_UID=1001

chmod 0750 config
chmod 0700 data
chmod 0640 config/config.toml
chmod 0600 .env
chmod 0400 config/master.key
sudo chown "root:${DMX_CONTAINER_UID}" config config/config.toml
sudo chown -R "${DMX_CONTAINER_UID}:${DMX_CONTAINER_UID}" config/master.key data

test "$(wc -c < config/master.key | tr -d ' ')" -eq 32
sudo stat -c '%u:%g %a %n' \
  data config config/config.toml config/master.key

docker compose pull panel
docker compose run --rm --entrypoint sh panel -c '
  id
  test -w /data
  test -r /config/config.toml
  test -r /config/master.key
  test "$(wc -c < /config/master.key | tr -d " ")" -eq 32
  echo "Permissions et montages OK"
'

docker compose up -d
docker compose logs --tail=100 panel
```

Pour `DMX_CONTAINER_UID=1001`, les permissions attendues sont `1001:1001 700` pour `data`, `0:1001 750` pour `config`, `0:1001 640` pour `config/config.toml` et `1001:1001 400` pour `config/master.key`. Remplacez `1001` par `10001` avec le Compose officiel.

Ne régénérez jamais `config/master.key` pour une installation existante : restaurez la clé originale, sinon les secrets déjà chiffrés dans SQLite deviendront illisibles.

Le fichier [docker-compose.yml](install/linux/docker-compose.yml) est autonome et modifiable sans cloner le dépôt. Il utilise le mode réseau hôte, recommandé lorsqu’il faut laisser DmxServerManager choisir librement les ports des jeux.

### Choisir le mode réseau Docker

Les deux modes suivants sont valides, mais ils ne se configurent pas de la même manière :

| Mode | Avantage | Contrainte |
|---|---|---|
| `network_mode: host` — Compose officiel | Les ports de jeux réservés par le panneau sont immédiatement utilisables, sans liste `ports:` dans Compose. | Le conteneur ne peut pas rejoindre un réseau Docker `proxied`. Un Traefik conteneurisé doit joindre le port `5500` de l’hôte par une route déjà disponible dans votre infrastructure. |
| Réseau bridge partagé avec Traefik | Fonctionne comme un Compose classique : labels Traefik, `expose: 5500` et réseau externe partagé. | Docker ne peut pas publier un nouveau port de jeu à chaud. Il faut déclarer à l’avance des plages TCP/UDP libres dans `ports:` puis choisir les ports des instances dans ces plages. |

Pour utiliser un Traefik existant sans mode hôte, retirez `network_mode: host` et utilisez par exemple cette adaptation :

```yaml
services:
  panel:
    networks:
      - proxied
    expose:
      - "5500"
    ports:
      - "5520-5530:5520-5530/udp"
      - "25600-25649:25600-25649/tcp"
      - "19140-19159:19140-19159/udp"
      - "24600-24649:24600-24649/udp"
      - "8220-8240:8220-8240/udp"
      - "27020-27119:27020-27119/tcp"
      - "27020-27119:27020-27119/udp"
    labels:
      traefik.enable: "true"
      traefik.docker.network: proxied
      traefik.http.routers.dmxmanager.rule: "Host(`${DMX_HOSTNAME}`)"
      traefik.http.routers.dmxmanager.entrypoints: websecure
      traefik.http.routers.dmxmanager.tls: "true"
      traefik.http.routers.dmxmanager.tls.certresolver: leresolver
      traefik.http.services.dmxmanager.loadbalancer.server.port: "5500"

networks:
  proxied:
    external: true
```

Dans ce mode bridge, configurez `bind = "0.0.0.0:5500"`, `reverse_proxy = true` et l’adresse IP exacte du conteneur Traefik dans `trusted_proxies`. Le port `5500` reste uniquement exposé au réseau Docker : ne l’ajoutez pas à `ports:`. Adaptez les plages de jeux aux ports libres de votre hôte ; une plage déjà utilisée empêchera le conteneur de démarrer.

### Méthode automatique optionnelle

Le script officiel réalise exactement cette initialisation :

```bash
curl -fsSLo /tmp/dmx-server-manager-install-docker.sh \
  https://github.com/thefrcrazy/DmxServerManager/releases/latest/download/dmx-server-manager-install-docker.sh \
  && sudo sh /tmp/dmx-server-manager-install-docker.sh
```

Accès local : `http://localhost:5500`. Le réseau hôte reste obligatoire pour les ports TCP/UDP dynamiques des jeux. Depuis un autre poste, utilisez temporairement un tunnel SSH :

```bash
ssh -L 5500:127.0.0.1:5500 user@server
```

Pour mettre à jour l’image et recréer le conteneur :

```bash
cd /opt/dmx-server-manager
docker compose pull panel
docker compose up -d --force-recreate panel
```

L’image suivie par défaut est `ghcr.io/thefrcrazy/dmx-server-manager:latest`; chaque release conserve aussi son tag versionné pour le rollback. Un simple `docker pull` ne remplace pas un conteneur déjà lancé, d’où la seconde commande Compose. Le projet ne fournit aucun Traefik : branchez votre reverse proxy HTTPS externe sur le port hôte `5500` après avoir configuré précisément `bind`, `reverse_proxy` et `trusted_proxies` dans `config/config.toml`.

L’équivalent avec `docker pull` explicite est `docker pull ghcr.io/thefrcrazy/dmx-server-manager:latest && docker compose up -d --force-recreate panel`, depuis `/opt/dmx-server-manager`.

Sauvegardez séparément `config/master.key` et `data/`. La clé ne doit jamais être placée dans une archive de données ni publiée.

## Installations natives

- Linux : [guide Linux et Docker](docs/INSTALLATION.md#linux-natif)
- Windows : [guide Windows](docs/INSTALLATION.md#windows-natif)
- Docker Desktop Windows : [contraintes spécifiques](docs/INSTALLATION.md#docker-desktop-windows)

Emplacements par défaut :

| Plateforme | Configuration | Données |
|---|---|---|
| Linux | `/etc/dmx-server-manager/config.toml` | `/var/lib/dmx-server-manager` |
| Windows | `%PROGRAMDATA%\DmxServerManager\config\config.toml` | `%PROGRAMDATA%\DmxServerManager\data` |
| Docker | `/config/config.toml` | `/data` |

## Sécurité

Une écoute non-loopback est refusée sans TLS ou reverse proxy explicitement déclaré. La clé maître XChaCha20-Poly1305 n’est stockée ni dans SQLite ni dans les sauvegardes. Les sessions utilisent des cookies opaques `HttpOnly`, `Secure` et `SameSite=Strict`; toutes les mutations exigent un jeton CSRF lié à la session.

Console, fichiers, mods et profils Steam sont des capacités à haut risque réservées à des opérateurs de confiance. Consultez le [modèle de sécurité](docs/SECURITY.md) avant toute exposition réseau.

## Documentation

- [Installation](docs/INSTALLATION.md)
- [Configuration](docs/CONFIGURATION.md)
- [Profils et ports](docs/GAME_PROFILES.md)
- [Exploitation, sauvegardes et dépannage](docs/OPERATIONS.md)
- [Tâches planifiées](docs/SCHEDULES.md)
- [Sécurité](docs/SECURITY.md)
- [Mises à niveau et rollback](docs/UPGRADING.md)

L’API stable est préfixée par `/api/v1`; sa description OpenAPI est fournie avec chaque release. Il n’existe aucun endpoint de compatibilité avec les versions antérieures.

## Développement

Prérequis : Rust 1.97.0 et Bun 1.3.14. Le verrou Cargo et `frontend/bun.lock` sont obligatoires.

```bash
cd backend
cargo test --locked --all-targets --all-features

cd ../frontend
bun install --frozen-lockfile
bun run lint
bun run test
bun run build
```

La CI contrôle aussi Clippy sans warning, les licences/advisories, l’audit Bun, les tests Playwright, le build Docker AMD64, le smoke test, Trivy et le SBOM SPDX. La configuration et les suites Playwright 1.0 sont obligatoires : leur absence ou un test en échec bloque la CI.

Les contributions doivent partir d’une branche dédiée et passer par une pull request. La publication est déclenchée uniquement par un tag `vX.Y.Z` correspondant à la version du crate et produit les binaires Linux/Windows, l’image GHCR, les checksums, le manifeste Ed25519, les signatures Sigstore et le SBOM.

## Licence

[MIT](LICENSE)
