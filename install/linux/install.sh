#!/bin/bash
# ========================================
# Draveur Manager - Script d'installation Linux
# ========================================

set -e

# Couleurs
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
NC='\033[0m'

# Variables
INSTALL_DIR="${DRAVEUR_INSTALL_DIR:-/opt/draveur-manager}"
DATA_DIR="${DRAVEUR_DATA_DIR:-/var/lib/draveur}"
USER="${DRAVEUR_USER:-draveur}"
REPO_URL="https://github.com/thefrcrazy/draveur-manager"

echo -e "${PURPLE}"
echo "╔═══════════════════════════════════════════════════════════╗"
echo "║              🎮 Draveur Manager Installer                 ║"
echo "║           Gestionnaire de serveurs Hytale                 ║"
echo "╚═══════════════════════════════════════════════════════════╝"
echo -e "${NC}"

# Vérification root
if [[ $EUID -ne 0 ]]; then
   echo -e "${RED}❌ Ce script doit être exécuté en tant que root${NC}"
   exit 1
fi

# Détection de la distribution
detect_distro() {
    if [ -f /etc/os-release ]; then
        . /etc/os-release
        DISTRO=$ID
    elif [ -f /etc/debian_version ]; then
        DISTRO="debian"
    elif [ -f /etc/redhat-release ]; then
        DISTRO="rhel"
    else
        DISTRO="unknown"
    fi
}

# Installation des dépendances
install_dependencies() {
    echo -e "${BLUE}📦 Installation des dépendances...${NC}"
    
    case $DISTRO in
        ubuntu|debian)
            apt-get update
            apt-get install -y curl git build-essential pkg-config libssl-dev
            ;;
        fedora|rhel|centos)
            dnf install -y curl git gcc openssl-devel
            ;;
        arch|manjaro)
            pacman -Sy --noconfirm curl git base-devel openssl
            ;;
        *)
            echo -e "${YELLOW}⚠️ Distribution non reconnue. Installez manuellement: curl, git, build-essential, openssl${NC}"
            ;;
    esac
}

# Installation de Rust
install_rust() {
    if command -v rustc &> /dev/null; then
        echo -e "${GREEN}✓ Rust déjà installé$(rustc --version)${NC}"
    else
        echo -e "${BLUE}🦀 Installation de Rust...${NC}"
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
    fi
}

# Installation de Bun
install_bun() {
    if command -v bun &> /dev/null; then
        echo -e "${GREEN}✓ Bun déjà installé $(bun --version)${NC}"
    else
        echo -e "${BLUE}🍞 Installation de Bun...${NC}"
        curl -fsSL https://bun.sh/install | bash
    fi
}

# Installation de Java 25 (pour Hytale)
install_java() {
    if java -version 2>&1 | grep -q "25"; then
        echo -e "${GREEN}✓ Java 25 déjà installé${NC}"
    else
        echo -e "${BLUE}☕ Installation de Java 25 (Adoptium Temurin)...${NC}"
        
        case $DISTRO in
            ubuntu|debian)
                apt-get install -y wget apt-transport-https
                wget -qO - https://packages.adoptium.net/artifactory/api/gpg/key/public | apt-key add -
                echo "deb https://packages.adoptium.net/artifactory/deb $(lsb_release -cs) main" > /etc/apt/sources.list.d/adoptium.list
                apt-get update
                apt-get install -y temurin-25-jdk || echo -e "${YELLOW}⚠️ Java 25 non disponible, installez-le manuellement${NC}"
                ;;
            *)
                echo -e "${YELLOW}⚠️ Installez Java 25 manuellement depuis https://adoptium.net${NC}"
                ;;
        esac
    fi
}

# Création de l'utilisateur
create_user() {
    if id "$USER" &>/dev/null; then
        echo -e "${GREEN}✓ Utilisateur $USER existe déjà${NC}"
    else
        echo -e "${BLUE}👤 Création de l'utilisateur $USER...${NC}"
        useradd -r -m -d /home/$USER -s /bin/bash $USER
    fi
}

# Clonage et compilation
build_project() {
    echo -e "${BLUE}📥 Téléchargement du projet...${NC}"
    
    if [ -d "$INSTALL_DIR" ]; then
        cd "$INSTALL_DIR"
        git pull
    else
        git clone "$REPO_URL" "$INSTALL_DIR"
        cd "$INSTALL_DIR"
    fi
    
    echo -e "${BLUE}🔨 Compilation du backend Rust...${NC}"
    cd backend
    source "$HOME/.cargo/env" 2>/dev/null || true
    cargo build --release
    
    echo -e "${BLUE}🎨 Build du frontend...${NC}"
    cd ../frontend
    export PATH="$HOME/.bun/bin:$PATH"
    bun install
    bun run build
    
    # Copier le frontend build vers le dossier static du backend
    mkdir -p ../backend/static
    cp -r dist/* ../backend/static/

    # Nettoyage
    echo -e "${BLUE}🧹 Nettoyage des fichiers de build...${NC}"
    rm -rf node_modules
}

# Configuration des répertoires
setup_directories() {
    echo -e "${BLUE}📁 Configuration des répertoires...${NC}"
    
    mkdir -p "$DATA_DIR"/{servers,backups,data}
    chown -R $USER:$USER "$DATA_DIR"
    chown -R $USER:$USER "$INSTALL_DIR"
}

# Création du fichier .env
create_env_file() {
    echo -e "${BLUE}⚙️ Configuration...${NC}"
    
    local IP=$(hostname -I | awk '{print $1}')
    local PROTOCOL="https"
    [[ "$USE_HTTPS" == "false" ]] && PROTOCOL="http"

    cat > "$DATA_DIR/.env" << EOF
HOST=0.0.0.0
PORT=5500
DATABASE_URL=sqlite:$DATA_DIR/data/database.db?mode=rwc
JWT_SECRET=$(openssl rand -base64 32)
SERVERS_DIR=$DATA_DIR/servers
BACKUPS_DIR=$DATA_DIR/backups
UPLOADS_DIR=$DATA_DIR/data/uploads
FRONTEND_URL=$PROTOCOL://$IP:5500
RUST_LOG=info
EOF

    mkdir -p "$DATA_DIR/data/uploads"
    chown -R $USER:$USER "$DATA_DIR"
    chown $USER:$USER "$DATA_DIR/.env"
    chmod 600 "$DATA_DIR/.env"
}

# Création du service systemd
create_systemd_service() {
    echo -e "${BLUE}🔧 Configuration du service systemd...${NC}"
    
    cat > /etc/systemd/system/draveur.service << EOF
[Unit]
Description=Draveur Manager - Game Server Manager
After=network.target

[Service]
Type=simple
User=$USER
WorkingDirectory=$INSTALL_DIR/backend
EnvironmentFile=$DATA_DIR/.env
ExecStart=$INSTALL_DIR/backend/target/release/draveur
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
EOF

    systemctl daemon-reload
    systemctl enable draveur
}

# Affichage des informations finales
print_success() {
    local IP=$(hostname -I | awk '{print $1}')
    
    echo -e "${GREEN}"
    echo "╔═══════════════════════════════════════════════════════════╗"
    echo "║           ✅ Installation terminée avec succès !          ║"
    echo "╚═══════════════════════════════════════════════════════════╝"
    echo -e "${NC}"
    echo ""
    echo -e "  ${BLUE}📡 Interface web:${NC} http://$IP:5500"
    echo ""
    echo -e "  ${YELLOW}Commandes utiles:${NC}"
    echo "    sudo systemctl start draveur    # Démarrer"
    echo "    sudo systemctl stop draveur     # Arrêter"
    echo "    sudo systemctl status draveur   # Status"
    echo "    sudo journalctl -u draveur -f   # Logs"
    echo ""
    echo -e "  ${PURPLE}Premier démarrage:${NC}"
    echo "    1. Démarrez le service: sudo systemctl start draveur"
    echo "    2. Accédez à http://$IP:5500"
    echo "    3. Créez votre compte admin"
    echo ""
}

# Main
main() {
    detect_distro
    echo -e "${BLUE}📋 Distribution détectée: $DISTRO${NC}"
    
    install_dependencies
    install_rust
    install_bun
    install_java
    create_user
    build_project
    setup_directories
    create_env_file
    create_systemd_service
    print_success
}

main
