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
- interface React responsive en français et en anglais, pilotée par les capacités de chaque profil.

## Profils 1.0

| Profil | Distribution | Ports par défaut | Particularités |
|---|---|---|---|
| Hytale | downloader officiel, Java 25 géré | UDP 5520 | OAuth device par instance et mise à jour atomique |
| Minecraft Java | Vanilla, Paper, Fabric, Forge, NeoForge, Spigot, Purpur et Quilt | TCP 25565 | version et loader épinglés, EULA explicite |
| Minecraft Bedrock | archive officielle Linux ou Windows | UDP 19132 et 19133 | mondes, allowlist, permissions et packs |
| Valheim | SteamCMD anonyme, AppID `896660` | UDP `N` et `N+1` | Crossplay optionnel et sauvegardes isolées |
| Palworld | SteamCMD anonyme, AppID `2394010` | UDP 8211 | paramètres INI sûrs, REST/RCON désactivés par défaut |
| Steam personnalisé | dépôt anonyme natif | déclarés par le profil | AppID numérique, exécutable relatif, aucun shell |

Les binaires de jeux ne sont jamais inclus dans l’image ou les releases. Les installateurs officiels et SteamCMD les téléchargent à la demande, selon leurs licences.

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

Le réseau hôte est obligatoire : les instances réservent des ports TCP/UDP dynamiques. Le panneau reste sur loopback tant qu’aucun reverse proxy HTTPS n’est déclaré. Le bootstrap exige `cosign` 3 afin d’authentifier le digest GHCR avant toute écriture locale.

```bash
cd install/linux
export DMX_VERSION='1.0.4'
export DMX_IMAGE='ghcr.io/thefrcrazy/dmx-server-manager@sha256:<digest-du-manifeste-signé>'
sudo --preserve-env=DMX_VERSION,DMX_IMAGE ./bootstrap-docker.sh direct
docker compose pull
docker compose up -d
```

Accès local : `http://localhost:5500`. Depuis un autre poste, utilisez temporairement un tunnel SSH :

```bash
ssh -L 5500:127.0.0.1:5500 user@server
```

Pour une exposition publique avec Let's Encrypt :

```bash
cd install/linux
DMX_DOMAIN=panel.example.com \
DMX_ACME_EMAIL=admin@example.com \
DMX_VERSION='1.0.4' \
DMX_IMAGE='ghcr.io/thefrcrazy/dmx-server-manager@sha256:<digest-du-manifeste-signé>' \
sudo --preserve-env=DMX_DOMAIN,DMX_ACME_EMAIL,DMX_VERSION,DMX_IMAGE ./bootstrap-docker.sh traefik
docker compose -f docker-compose.traefik.yml up -d
```

L’image est `ghcr.io/thefrcrazy/dmx-server-manager`. Un volume nommé conserve `/data`; `/imports` est monté en lecture seule. Conservez `install/linux/secrets/master.key` hors des sauvegardes de données.

## Installations natives

- Linux : [guide Linux et Docker](docs/INSTALLATION.md#linux-natif)
- Windows : [guide Windows](docs/INSTALLATION.md#windows-natif)
- Docker Desktop Windows : [contraintes spécifiques](docs/INSTALLATION.md#docker-desktop-windows)

Emplacements par défaut :

| Plateforme | Configuration | Données |
|---|---|---|
| Linux | `/etc/dmx-server-manager/config.toml` | `/var/lib/dmx-server-manager` |
| Windows | `%PROGRAMDATA%\DmxServerManager\config\config.toml` | `%PROGRAMDATA%\DmxServerManager\data` |
| Docker | `/data/config.toml` | `/data` |

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
