# Profils de jeux et ports

| Profil | Distribution | Ports par défaut | Arrêt |
|---|---|---|---|
| Hytale | downloader officiel, Java 25 géré | UDP 5520 | protocole du driver |
| Minecraft Java | fournisseur/loader officiel | TCP 25565 | commande `stop` |
| Minecraft Bedrock | archive officielle Windows/Linux vérifiée par SHA-256 | UDP 19132 et 19133 | commande `stop` |
| Valheim | SteamCMD anonyme, AppID `896660` | UDP `N` et `N+1` | Ctrl+C |
| Palworld | SteamCMD anonyme, AppID `2394010` | UDP 8211 | arrêt contrôlé |
| Satisfactory | SteamCMD anonyme, AppID `1690800` | TCP/UDP 7777, TCP 8888 | interruption native |
| 7 Days to Die | SteamCMD anonyme, AppID `294420` | TCP/UDP 26900, UDP 26901-26902 | commande `shutdown` |
| Project Zomboid | SteamCMD anonyme, AppID `380870` | UDP 16261, 8766-8767 | commande `quit`, Linux AMD64 |
| Rust | SteamCMD anonyme, AppID `258550` | UDP 28015/28017, TCP 28016 | interruption native |
| Steam personnalisé | dépôt anonyme natif | déclaré par profil | stdin ou signal déclaré |

Les ports sont réservés en SQLite puis vérifiés contre l’hôte avant démarrage. DmxServerManager ne modifie jamais le pare-feu; ouvrez uniquement les ports des instances réellement publiées.

Un profil intégré est immuable. Une instance reste liée à une révision précise; une montée de version est explicite. Minecraft ne change jamais automatiquement de version majeure ou de loader.

## Hytale

L’installation utilise exclusivement le [downloader officiel Hytale](https://support.hytale.com/hc/en-us/articles/45326769420827-Hytale-Server-Manual). Le job passe à `waiting_for_user` pendant l’authentification device. Le downloader utilise `https://oauth.accounts.hytale.com/oauth2/device/verify` ; cette route est distincte de `https://accounts.hytale.com/device`, utilisée par l’authentification du serveur de jeu. DmxServerManager ne remplace plus l’une par l’autre : il conserve l’URL complète émise par le downloader ou ajoute le `user_code` à sa route OAuth exacte, sans modifier la casse du code. Lorsqu’une nouvelle demande est émise, elle remplace atomiquement l’ancienne dans le Job et dans l’instance. Les jetons persistants sont chiffrés dans le magasin de secrets ; le fichier temporaire du downloader est supprimé avant la fin du job.

La page Hytale utilise une protection navigateur. Un bloqueur de contenu peut empêcher l’affichage du challenge même avec une URL valide : autorisez temporairement `accounts.hytale.com` et `oauth.accounts.hytale.com`, ouvrez le lien dans un onglet normal, vérifiez que le code affiché correspond exactement à la carte du job, puis approuvez l’écran des permissions suivant.

Le driver impose Java 25, lance `HytaleServer.jar` depuis `game/Server/` avec `../Assets.zip` et n’exécute aucun script fourni par le serveur. DMX télécharge et vérifie Eclipse Temurin Java 25 dans son toolchain géré, vérifie encore `java -version` avant chaque lancement, puis l’utilise pour l’instance. Les bibliothèques natives JLine, Jansi et Netty QUIC sont extraites dans `.dmx-runtime/hytale-native`, répertoire privé recréé à chaque démarrage sur le volume exécutable de l’instance. `/tmp` peut donc rester monté `noexec` dans un déploiement durci.

Le profil Hytale expose également les options officielles `--allow-op`, `--disable-sentry`, `--accept-early-plugins`, ainsi que les sauvegardes natives et leur fréquence. Les évolutions compatibles du profil sont adoptées lorsqu’un nouveau paramètre est sauvegardé, sans réinstaller le jeu. `HytaleServer.aot` reste un cache de démarrage optionnel : DMX l’active uniquement lorsqu’il est fourni par l’archive officielle, sans rejeter les layouts officiels qui l’omettent. Une sortie documentée avec le code `8` adopte uniquement un arbre complet validé depuis `game/updater/staging`. La bascule conserve mondes, configuration et mods, maintient un rollback persistant, puis confirme la mise à jour après 30 secondes stables. Un crash pendant cette fenêtre restaure automatiquement la version précédente sans perdre les changements de monde intervenus pendant l’essai.

## Minecraft Java

La création n’affiche qu’un seul profil `Minecraft Java`. Le loader — Vanilla, Paper, Fabric, Forge, NeoForge, Spigot, Purpur ou Quilt — et la version sont des sous-configurations de cette instance. Les anciens identifiants de profils restent chargés uniquement afin que les instances créées avant la version 1.0.15 continuent de fonctionner; ils ne sont plus proposés dans le catalogue.

Chaque instance fixe une version exacte de Minecraft. L’écran de création interroge les catalogues officiels et propose les versions de jeu et de loader compatibles dans des sélecteurs. Fabric, Forge, NeoForge, Quilt et Purpur exigent aussi `loader_version`; cette valeur désigne respectivement une version exacte du loader ou un numéro de build Purpur. Les alias flottants tels que `latest`, `recommended` et `stable` sont refusés. Spigot n’expose pas ce champ : la version de BuildTools est une constante de maintenance du panneau. Seul un changement de version, de loader ou de version du loader exige une réinstallation. Un changement de port, mémoire ou autre réglage appliqué au démarrage ne remet pas l’instance en état « à réinstaller ».

| Variante | Source et validation | Contenu utilisateur |
|---|---|---|
| Vanilla | manifeste Mojang, URL et SHA-1 officiels | mondes et configuration |
| Paper | API officielle, version et build exacts | `plugins/`, mondes et fichiers Paper |
| Fabric | paire Minecraft/loader vérifiée dans Fabric Meta; installer `1.1.1` épinglé par taille et SHA-256 | `mods/`, `config/`, `defaultconfigs/` et mondes |
| Forge | installateur Maven exact, sidecar SHA-256 et version Minecraft vérifiée dans `install_profile.json` | `mods/`, `config/`, `defaultconfigs/` et mondes |
| NeoForge | mêmes contrôles Maven; cohérence entre la ligne NeoForge et la version Minecraft | `mods/`, `config/`, `defaultconfigs/` et mondes |
| Quilt | paire Minecraft/loader vérifiée dans Quilt Meta; installer `0.15.0` épinglé par taille et SHA-256 | `mods/`, `config/`, `defaultconfigs/` et mondes |
| Spigot | BuildTools Jenkins `#200` immuable, taille et SHA-256 épinglés | `plugins/`, mondes et fichiers Bukkit/Spigot |
| Purpur | version et build exacts via l’API v2; MD5 fournisseur vérifié puis SHA-256 local enregistré | `plugins/`, mondes et fichiers Paper/Purpur |

Pins mainteneur actuellement livrés :

- Fabric Installer `1.1.1` : SHA-256 `2487a69dd6f9d9c2605265a7142d77c26ab62edc620e6bcf810d581d2ee31b79`, 209 151 octets.
- Quilt Installer `0.15.0` : SHA-256 `f0c6e04e7f3b932d801b9e783ae17c960ff3cadc0f0109d6cc9be5240e99d455`, 7 381 964 octets.
- Spigot BuildTools Jenkins `#200` : SHA-256 `b61fa90158f594ee95bea1a27399eb64d439b4c8ae9345bd4476a02ce49b06ff`, 3 606 248 octets.

Avant une release, les contrats réseau peuvent être vérifiés sans accepter d’EULA, démarrer un serveur ni télécharger les distributions de jeux :

```bash
cd backend
cargo test --locked live_official -- --ignored
cargo test --locked live_modrinth_metadata_contract_is_compatible -- --ignored
```

Ces smokes interrogent les métadonnées Mojang, Paper, Fabric, Quilt, Forge, NeoForge, Purpur et Modrinth, téléchargent les petits bootstrap/installateurs épinglés Fabric, Quilt, BuildTools et Hytale, puis appliquent les mêmes parseurs, allowlists, tailles et checksums que le runtime. Ils restent ignorés dans la suite hors-ligne normale.

Le runtime Java est choisi depuis les métadonnées officielles de la version Minecraft, puis installé dans le magasin de toolchains géré. Les installateurs sont lancés directement avec un tableau d’arguments, un environnement filtré, une durée maximale et un groupe de processus contenu. Aucun `run.sh`, `run.bat`, shell ou argument fourni par l’API n’est exécuté. Forge ancien peut démarrer avec son JAR historique exact lorsque l’installateur ne produit pas d’argfile; Forge moderne et NeoForge utilisent uniquement l’argfile officiel relatif après validation. Les argfiles imbriqués, chemins absolus, traversals et options Java déclenchant une commande hôte sont refusés.

BuildTools nécessite les outils de compilation qu’il documente, notamment Git, sur une installation native. Une absence de prérequis fait échouer le job sans adopter le staging. Purpur ne publie actuellement qu’un MD5 dans son API : ce MD5 sert à détecter une corruption du téléchargement, pas à établir une provenance cryptographique; le panneau calcule ensuite un SHA-256 pour son propre inventaire.

SteamCMD est toujours lancé depuis le chemin absolu déclaré par l’administrateur avec `+login anonymous`, un AppID numérique et un tableau d’arguments construit par le backend. Aucun identifiant Steam privé n’est accepté. Les profils Valheim, Palworld et Steam personnalisé restent indisponibles si la toolchain native n’est pas installée; le panneau ne remplace jamais ce chemin par une commande issue de l’API.

Une importation existante ne fait confiance ni au nom du fichier ni au contenu du marqueur fourni. Elle vérifie la compatibilité auprès du fournisseur pour Fabric, Quilt et Purpur, la cohérence de version pour Forge/NeoForge, l’arbre complet sans lien ni fichier spécial et les launchers/argfiles attendus. Les imports Forge anciens sans argfile ne sont pas acceptés par l’API 1.0 et doivent être réinstallés dans le stockage géré.

Références primaires : [Fabric Server](https://fabricmc.net/use/server/), [Fabric Meta](https://meta.fabricmc.net/), [Quilt Server](https://quiltmc.org/en/install/server/), [Forge](https://files.minecraftforge.net/net/minecraftforge/forge/), [NeoForge Server](https://docs.neoforged.net/docs/gettingstarted/server/), [Spigot BuildTools](https://www.spigotmc.org/wiki/buildtools/), [Purpur API](https://purpurmc.org/docs/purpur/api/).

## Minecraft Bedrock

Le panneau interroge l’endpoint public de téléchargement utilisé par le site officiel Minecraft, sélectionne uniquement `serverBedrockLinux` ou `serverBedrockWindows`, puis refuse toute URL qui ne correspond pas exactement au chemin HTTPS officiel attendu. Microsoft ne publie pas de checksum dans cette réponse : DmxServerManager calcule donc le SHA-256 du fichier reçu, valide la taille, la structure ZIP et l’exécutable avant de l’enregistrer dans l’inventaire. Une source administrateur épinglée avec SHA-256 reste prioritaire lorsqu’elle est configurée.

Le profil prend en charge les archives officielles AMD64 Linux et Windows. L’EULA doit être acceptée explicitement à la création et cette décision est auditée. Si la découverte officielle échoue, le même job attend qu’un Owner fournisse l’archive officielle et son SHA-256 ; un nouvel import générique ne peut pas contourner ce contrôle.

Les mises à jour utilisent un staging puis une bascule avec rollback. `server.properties` conserve les clés et commentaires inconnus. Les mondes, `allowlist.json`, `permissions.json`, `behavior_packs/` et `resource_packs/` sont préservés avec des quotas et sans suivre de liens. Le lancement utilise exclusivement `bedrock_server` ou `bedrock_server.exe` du stockage de l’instance, sans shell, puis l’arrêt envoie `stop` sur stdin. Les sauvegardes sont pilotées par le profil et excluent binaires, caches, logs et secrets.

Références primaires : [téléchargement officiel](https://www.minecraft.net/en-us/download/server/bedrock), [prise en main Microsoft Learn](https://learn.microsoft.com/en-us/minecraft/creator/documents/bedrockserver/getting-started?view=minecraft-bedrock-stable), [propriétés serveur](https://learn.microsoft.com/en-us/minecraft/creator/documents/bedrockserver/server-properties?view=minecraft-bedrock-stable), [commande stop](https://learn.microsoft.com/en-us/minecraft/creator/commands/commands/stop?view=minecraft-bedrock-stable).

## Valheim

Le profil installe anonymement l’AppID SteamCMD `896660` dans le stockage géré et refuse de démarrer si l’exécutable natif attendu manque. La configuration fixe le nom du serveur, le monde, un port UDP de base `N`, le port UDP adjacent `N+1` et un mot de passe de 5 à 64 caractères. Le mot de passe est chiffré dans le magasin de secrets, masqué dans l’API et expurgé des logs. L’option Crossplay ajoute uniquement l’argument officiel correspondant ; elle ne modifie ni le pare-feu ni la configuration réseau de l’hôte.

Les mondes sont isolés dans le répertoire `data/` de l’instance au moyen de `-savedir`. Une sauvegarde demandée pendant l’exécution interrompt proprement le serveur avec un délai maximal de 60 secondes, archive uniquement ce répertoire, puis relance l’instance si elle était auparavant en fonctionnement. Ouvrez les deux ports UDP réservés et vérifiez leur accessibilité depuis l’extérieur ; ne réutilisez pas le port `N+1` pour une autre instance.

Référence primaire : [guide officiel des serveurs dédiés Valheim](https://valheim.com/support/a-guide-to-dedicated-servers/).

## Palworld

Le profil installe anonymement l’AppID SteamCMD `2394010` et valide `PalServer.sh` sur Linux ou `PalServer.exe` sur Windows. Il expose le nom du serveur, le port UDP — `8211` par défaut —, la publication facultative dans la liste publique et des mots de passe serveur/administrateur chiffrés. DmxServerManager met à jour `PalWorldSettings.ini` par remplacement atomique, conserve les options inconnues déjà valides et n’injecte jamais ces valeurs dans une commande shell.

Les sauvegardes couvrent uniquement `game/Pal/Saved`. Si le serveur tourne, le superviseur lui envoie d’abord l’interruption native avec un délai maximal de 60 secondes afin qu’il vide son monde sur disque, puis le redémarre après l’archive. Le profil 1.0 n’active ni ne configure REST/RCON et ne les utilise pas comme mécanismes de gestion. Le mode serveur public ne crée aucune règle de pare-feu : ouvrez explicitement le port UDP réservé si l’instance doit être joignable depuis Internet.

Référence primaire : [guide officiel du serveur dédié Palworld](https://docs.palworldgame.com/getting-started/deploy-dedicated-server/).

## Satisfactory

Le profil installe anonymement l’AppID SteamCMD `1690800` et valide `FactoryServer.sh` sous Linux ou `FactoryServer.exe` sous Windows. Il réserve le port principal en TCP et UDP — `7777` par défaut — ainsi que le port de messagerie fiable TCP — `8888` par défaut. Les arguments `-Port`, `-ReliablePort` et `-ExternalReliablePort` sont construits par le backend et ne passent jamais par un shell.

Les sauvegardes couvrent uniquement `game/FactoryGame/Saved`. Le nom et le mot de passe administrateur sont réclamés depuis le jeu lors de la première prise de contrôle du serveur, conformément au fonctionnement officiel, et ne sont donc pas simulés par des options sans effet dans le panneau.

Référence primaire : [wiki officiel Satisfactory — serveurs dédiés](https://satisfactory.wiki.gg/wiki/Dedicated_servers).

## 7 Days to Die

Le profil installe anonymement l’AppID SteamCMD `294420`, lance directement `7DaysToDieServer.x86_64` ou `7DaysToDieServer.exe` et génère atomiquement `dmx-serverconfig.xml`. Les ports `26900`, `26901` et `26902` doivent rester adjacents. Le mot de passe est chiffré; Telnet et le dashboard web restent désactivés afin de ne pas exposer une seconde surface d’administration.

Les données persistantes sont forcées sous `data/7days-to-die`, en dehors des binaires remplaçables. L’arrêt utilise la commande console `shutdown`; les sauvegardes incluent les données, le XML géré et le répertoire `Mods` s’il existe.

## Project Zomboid

Le profil installe anonymement l’AppID SteamCMD `380870`. Il est limité à Linux AMD64 car Steam déclare `start-server.sh` comme point d’entrée Linux mais uniquement des fichiers batch sous Windows; DmxServerManager n’exécute volontairement ni shell arbitraire ni `.bat`. Le nom interne est limité aux caractères alphanumériques, tiret et underscore, et le mot de passe administrateur est chiffré.

Le profil utilise les ports UDP `16261`, `8766` et `8767`, place `HOME` dans le stockage isolé de l’instance et écrit la configuration sous `data/Zomboid/Server`. L’arrêt envoie `quit` sur stdin et les sauvegardes couvrent `data/Zomboid`.

## Rust

Le profil installe anonymement l’AppID SteamCMD `258550` et lance directement `RustDedicated` ou `RustDedicated.exe`. Le backend construit une liste fermée d’arguments pour le port de jeu UDP `28015`, le query port UDP `28017`, le RCON TCP `28016`, le nom, l’identité, la limite de joueurs, la taille et la graine du monde. Le mot de passe RCON est chiffré et expurgé des logs.

Les sauvegardes couvrent le répertoire `game/server`, qui contient mondes, configuration, utilisateurs et identité de l’instance. DmxServerManager n’installe pas automatiquement uMod/Carbon et ne modifie pas le pare-feu.

Référence primaire : [wiki officiel Facepunch — création d’un serveur Rust](https://wiki.facepunch.com/rust/Creating-a-server).

## Steam personnalisé

Le profil accepte un AppID numérique, une branche publique optionnelle, un exécutable relatif, un tableau d’arguments typés, les ports et les chemins de sauvegarde. Chemins absolus, shell, URL arbitraire, exécutable hôte et compte Steam privé sont refusés. L’installation échoue clairement si le dépôt anonyme, l’OS ou AMD64 ne sont pas compatibles.

## Mods

L’onglet est affiché uniquement si le profil le déclare : Modrinth/import manuel et CurseForge optionnel pour les loaders Minecraft compatibles; plugins pour Paper/Purpur/Spigot; import manuel Hytale. Pour Bedrock, les packs restent gérés manuellement via le gestionnaire de fichiers sécurisé tant qu’aucun workflow dédié complet n’est exposé. Valheim, Palworld, Bedrock et Steam générique masquent donc l’onglet Mods par défaut.
