/**
 * Commandes de console documentées par jeu.
 *
 * Elles alimentent les suggestions du terminal : sans elles, la complétion ne
 * pouvait proposer que ce qui avait déjà été saisi sur l'instance, donc rien du
 * tout au premier usage — précisément le moment où l'aide compte le plus.
 *
 * Seules figurent ici les commandes publiées par l'éditeur ou imprimées par le
 * serveur lui-même. La liste est volontairement incomplète plutôt
 * qu'approximative : une suggestion inventée coûte plus qu'une suggestion
 * absente, puisque `help` reste toujours proposée pour obtenir la liste réelle.
 */
export interface GameCommand {
    /** Saisie insérée dans le champ, arguments compris. */
    command: string;
    /** Résumé court, affiché par le navigateur à côté de la suggestion. */
    hint: string;
}

const MINECRAFT_JAVA: GameCommand[] = [
    { command: "help", hint: "Liste les commandes disponibles" },
    { command: "list", hint: "Joueurs connectés" },
    { command: "say ", hint: "Message à tous les joueurs" },
    { command: "save-all", hint: "Écrit le monde sur disque" },
    { command: "save-off", hint: "Suspend les sauvegardes automatiques" },
    { command: "save-on", hint: "Reprend les sauvegardes automatiques" },
    { command: "stop", hint: "Arrête proprement le serveur" },
    { command: "op ", hint: "Donne les droits opérateur" },
    { command: "deop ", hint: "Retire les droits opérateur" },
    { command: "kick ", hint: "Expulse un joueur" },
    { command: "ban ", hint: "Bannit un joueur" },
    { command: "pardon ", hint: "Lève un bannissement" },
    { command: "whitelist on", hint: "Active la liste blanche" },
    { command: "whitelist off", hint: "Désactive la liste blanche" },
    { command: "whitelist add ", hint: "Ajoute un joueur à la liste blanche" },
    { command: "whitelist reload", hint: "Recharge la liste blanche" },
    { command: "gamemode ", hint: "Change le mode de jeu" },
    { command: "difficulty ", hint: "Change la difficulté" },
    { command: "time set day", hint: "Passe au jour" },
    { command: "weather clear", hint: "Dégage le ciel" },
    { command: "gamerule ", hint: "Modifie une règle de jeu" },
    { command: "seed", hint: "Affiche la graine du monde" },
];

const MINECRAFT_BEDROCK: GameCommand[] = [
    { command: "help", hint: "Liste les commandes disponibles" },
    { command: "list", hint: "Joueurs connectés" },
    { command: "say ", hint: "Message à tous les joueurs" },
    { command: "save hold", hint: "Prépare une sauvegarde cohérente" },
    { command: "save query", hint: "État de la sauvegarde en cours" },
    { command: "save resume", hint: "Reprend les écritures" },
    { command: "stop", hint: "Arrête proprement le serveur" },
    { command: "op ", hint: "Donne les droits opérateur" },
    { command: "deop ", hint: "Retire les droits opérateur" },
    { command: "kick ", hint: "Expulse un joueur" },
    { command: "allowlist on", hint: "Active la liste d'autorisation" },
    { command: "allowlist off", hint: "Désactive la liste d'autorisation" },
    { command: "allowlist add ", hint: "Autorise un joueur" },
    { command: "allowlist reload", hint: "Recharge la liste d'autorisation" },
    { command: "gamemode ", hint: "Change le mode de jeu" },
    { command: "difficulty ", hint: "Change la difficulté" },
    { command: "time set day", hint: "Passe au jour" },
    { command: "gamerule ", hint: "Modifie une règle de jeu" },
];

// Le serveur Hytale imprime lui-même les deux commandes de mise à jour dans sa
// console. Le reste n'est pas encore publié : `/help` fait foi sur l'instance.
const HYTALE: GameCommand[] = [
    { command: "/help", hint: "Liste les commandes de cette version" },
    { command: "/update check", hint: "Interroge le fournisseur sur la version disponible" },
    { command: "/update download", hint: "Prépare la mise à jour téléchargée" },
    { command: "/stop", hint: "Arrête proprement le serveur" },
];

const VALHEIM: GameCommand[] = [
    { command: "help", hint: "Liste les commandes disponibles" },
    { command: "info", hint: "Mémoire et état du serveur" },
    { command: "save", hint: "Écrit le monde sur disque" },
    { command: "kick ", hint: "Expulse un joueur (nom, IP ou identifiant)" },
    { command: "ban ", hint: "Bannit un joueur" },
    { command: "unban ", hint: "Lève un bannissement" },
    { command: "banned", hint: "Liste les joueurs bannis" },
    { command: "ping", hint: "Latence vers le serveur" },
    { command: "shutdown", hint: "Arrête proprement le serveur" },
];

const PALWORLD: GameCommand[] = [
    { command: "/Info", hint: "Version et nom du serveur" },
    { command: "/ShowPlayers", hint: "Joueurs connectés et leurs identifiants" },
    { command: "/Save", hint: "Écrit le monde sur disque" },
    { command: "/Broadcast ", hint: "Message à tous les joueurs" },
    { command: "/KickPlayer ", hint: "Expulse un joueur (SteamID)" },
    { command: "/BanPlayer ", hint: "Bannit un joueur (SteamID)" },
    { command: "/TeleportToPlayer ", hint: "Se téléporte vers un joueur" },
    { command: "/TeleportToMe ", hint: "Téléporte un joueur vers soi" },
    { command: "/Shutdown 60 ", hint: "Arrêt différé avec message" },
    { command: "/DoExit", hint: "Arrêt immédiat" },
];

const SEVEN_DAYS_TO_DIE: GameCommand[] = [
    { command: "help", hint: "Liste les commandes disponibles" },
    { command: "lp", hint: "Joueurs connectés" },
    { command: "say ", hint: "Message à tous les joueurs" },
    { command: "saveworld", hint: "Écrit le monde sur disque" },
    { command: "kick ", hint: "Expulse un joueur" },
    { command: "ban add ", hint: "Bannit un joueur" },
    { command: "admin add ", hint: "Donne les droits administrateur" },
    { command: "gettime", hint: "Heure de jeu courante" },
    { command: "settime ", hint: "Change l'heure de jeu" },
    { command: "version", hint: "Version du serveur" },
    { command: "shutdown", hint: "Arrête proprement le serveur" },
];

const PROJECT_ZOMBOID: GameCommand[] = [
    { command: "/help", hint: "Liste les commandes disponibles" },
    { command: "/players", hint: "Joueurs connectés" },
    { command: "/save", hint: "Écrit le monde sur disque" },
    { command: "/servermsg ", hint: "Message à tous les joueurs" },
    { command: "/kick ", hint: "Expulse un joueur" },
    { command: "/banuser ", hint: "Bannit un joueur" },
    { command: "/unbanuser ", hint: "Lève un bannissement" },
    { command: "/adduser ", hint: "Crée un compte joueur" },
    { command: "/setaccesslevel ", hint: "Change le niveau d'accès" },
    { command: "/checkModsNeedUpdate", hint: "Vérifie les mods de l'atelier" },
    { command: "/quit", hint: "Sauvegarde puis arrête le serveur" },
];

const RUST: GameCommand[] = [
    { command: "status", hint: "Joueurs connectés et état du serveur" },
    { command: "say ", hint: "Message à tous les joueurs" },
    { command: "save", hint: "Écrit le monde sur disque" },
    { command: "kick ", hint: "Expulse un joueur" },
    { command: "ban ", hint: "Bannit un joueur" },
    { command: "server.writecfg", hint: "Écrit la configuration serveur" },
    { command: "quit", hint: "Arrête proprement le serveur" },
];

const CATALOG: Record<string, GameCommand[]> = {
    hytale: HYTALE,
    "minecraft-java": MINECRAFT_JAVA,
    "minecraft-bedrock": MINECRAFT_BEDROCK,
    valheim: VALHEIM,
    palworld: PALWORLD,
    "seven-days-to-die": SEVEN_DAYS_TO_DIE,
    "project-zomboid": PROJECT_ZOMBOID,
    rust: RUST,
};

/**
 * Commandes suggérées pour un profil, y compris les déclinaisons à moddeurs
 * (`minecraft-java-fabric`, `minecraft-java-paper`…) qui partagent la console
 * du serveur vanilla. Un profil inconnu ne propose rien plutôt que d'induire en
 * erreur avec les commandes d'un autre jeu.
 */
export function gameCommands(profileId: string): GameCommand[] {
    if (profileId.startsWith("minecraft-java")) return MINECRAFT_JAVA;
    return CATALOG[profileId] ?? [];
}
