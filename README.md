# DmxServerManager

[![CI](https://github.com/thefrcrazy/DmxServerManager/actions/workflows/ci.yml/badge.svg)](https://github.com/thefrcrazy/DmxServerManager/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Gestionnaire mono-hôte de serveurs de jeux, écrit en Rust et React. La version 1.0 cible Linux AMD64 natif sur Ubuntu 24.04/glibc 2.39+, Windows AMD64 et les conteneurs Linux AMD64. Elle utilise SQLite, des comptes locaux et des sauvegardes locales.

## Profils 1.0

- Hytale
- Minecraft Java : Vanilla, Paper, Fabric, Forge, NeoForge, Spigot, Purpur et Quilt
- Minecraft Bedrock
- Valheim
- Palworld
- serveurs SteamCMD anonymes personnalisés, avec compatibilité « best effort »

Les binaires de jeux ne sont jamais inclus dans l’image ou les releases. Les installateurs officiels et SteamCMD les téléchargent à la demande, selon leurs licences.

Les builds officiels surveillent par défaut un manifeste de release Ed25519 avec checksums complets, à partir d’une URL et d’une clé publique intégrées au binaire. Le panneau affiche une procédure native ou une image Docker épinglée par digest uniquement pour une version strictement plus récente ; il n’exécute jamais la mise à niveau et ne se remplace pas lui-même.

## Installation rapide avec Docker

Le réseau hôte est obligatoire : les instances réservent des ports TCP/UDP dynamiques. Le panneau reste sur loopback tant qu’aucun reverse proxy HTTPS n’est déclaré. Le bootstrap exige `cosign` 3 afin d’authentifier le digest GHCR avant toute écriture locale.

```bash
cd install/linux
export DMX_VERSION='1.0.0'
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
DMX_VERSION='1.0.0' \
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
bun run build
```

La CI contrôle aussi Clippy sans warning, les licences/advisories, l’audit Bun, les tests Playwright, le build Docker AMD64, le smoke test, Trivy et le SBOM SPDX. La configuration et les suites Playwright 1.0 sont obligatoires : leur absence ou un test en échec bloque la CI.

## Licence

[MIT](LICENSE)
