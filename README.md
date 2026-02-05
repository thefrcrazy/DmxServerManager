# 🎮 Draveur Manager

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Backend-Rust-orange.svg)](https://www.rust-lang.org/)
[![React](https://img.shields.io/badge/Frontend-React-blue.svg)](https://react.dev/)

**Gestionnaire de serveurs de jeux moderne et performant** — Inspiré de [Crafty Controller](https://craftycontrol.com/), conçu pour Hytale et au-delà.

## 🚧 Statut du projet

Ce projet est actuellement **en cours de développement** (WORK IN PROGRESS).

Des **fichiers de release** (binaires/archives) seront mis à disposition une fois une version stable finalisée.

![Dashboard Preview](docs/assets/dashboard-preview.png)

---

## ✨ Fonctionnalités

- 🖥️ **Interface Web Premium** — Dashboard moderne avec SCSS, animations fluides
- 🎮 **Multi-Serveurs** — Gérez plusieurs serveurs depuis une interface unique
- 📺 **Console Live** — WebSocket temps réel pour les logs et commandes
- 💾 **Backups Automatiques** — Sauvegardes planifiées avec compression
- 🔔 **Discord Webhooks** — Notifications enrichies
- ⏰ **Tâches Planifiées** — Redémarrages, mises à jour automatiques
- 🔐 **Authentification JWT** — Sécurisé avec gestion des rôles
- 🐳 **Docker Ready** — Déploiement simplifié

---

## 🚀 Installation

### Linux (Docker)

Plusieurs variantes de déploiement sont disponibles selon vos besoins.

#### 1. Standard (HTTPS Auto-signé) — Rapide
Idéal pour un usage sur serveur distant via IP directe. Par défaut, l'installation Docker utilise le HTTPS auto-signé pour chiffrer les communications.

```bash
docker compose -f install/linux/docker-compose.yml up -d
```
*Note : Le navigateur affichera une alerte de sécurité au premier accès, c'est normal.*

**Accès HTTP (Désactiver HTTPS) :**
Si vous préférez le HTTP simple, ajoutez `USE_HTTPS=false` dans votre `.env`.

#### 2. Traefik (HTTPS Automatique) — Recommandé
Gère automatiquement vos certificats SSL via Let's Encrypt.
**Prérequis :** Créer un fichier `.env` avec vos infos :
```bash
DOMAIN_NAME=panel.votre-domaine.com
ACME_EMAIL=votre@email.com
```
Lancer l'installation :
```bash
docker compose -f install/linux/docker-compose.traefik.yml up -d
```

### Linux (Sans Docker)

```bash
git clone https://github.com/thefrcrazy/draveur-manager.git
cd draveur-manager
./install/linux/install.sh
```

### Windows

```powershell
# Exécuter PowerShell en Administrateur
irm https://raw.githubusercontent.com/thefrcrazy/draveur-manager/main/install/windows/install.ps1 | iex
```

---

## 📖 Documentation

- [Guide d'Installation Complet](docs/INSTALL.md)
- [Configuration des Serveurs](docs/SERVERS.md)
- [API Reference](docs/API.md)

---

## 🛠️ Stack Technique

| Composant            | Technologie                      |
| -------------------- | -------------------------------- |
| **Frontend**         | React + Vite + TypeScript + SCSS |
| **Backend**          | Rust + Axum                 |
| **Base de données**  | SQLite                           |
| **Runtime**          | Bun (frontend), Tokio (backend)  |
| **Containerisation** | Docker + Docker Compose          |

---

## 🎯 Roadmap

- [x] Structure du projet
- [x] Docker Compose
- [x] Backend API REST
- [x] Console WebSocket
- [x] Interface Dashboard
- [x] Configuration Dashboard
- [ ] Support Hytale
    - [x] Gestion Installation / Edit / Delete
    - [x] Gestion Console
    - [x] Gestion Log
    - [ ] Intégrations backups
    - [ ] Planification Tâches
    - [x] Meilleur FTP interne
    - [ ] Configuration temps réel avec les fichiers server
    - [ ] Gestion des mods ([CurseForge](https://www.curseforge.com/hytale/search?class=mods))
    - [x] Meilleur affichage joueurs
    - [x] Ajout des métriques
    - [ ] Gestion webhook
- [ ] Support Minecraft
- [ ] Support Palworld
- [ ] Support Valheim
- [ ] Support Custom Steam Server

---

## 🤝 Contribution

Les contributions sont les bienvenues ! Voir [CONTRIBUTING.md](CONTRIBUTING.md).

---

## 📜 Licence

Ce projet est sous licence [MIT](LICENSE).

---

## 🙏 Crédits

- Inspiré par [Crafty Controller](https://craftycontrol.com/)
- Basé sur [hytale-server](https://github.com/thefrcrazy/hytale-server)
