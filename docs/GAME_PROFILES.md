# Profils de jeux et ports

| Profil | Distribution | Ports par défaut | Arrêt |
|---|---|---|---|
| Hytale | downloader officiel, Java 25 géré | UDP 5520 | protocole du driver |
| Minecraft Java | fournisseur/loader officiel | TCP 25565 | commande `stop` |
| Minecraft Bedrock | archive officielle Windows/Linux vérifiée par SHA-256 | UDP 19132 et 19133 | commande `stop` |
| Valheim | SteamCMD anonyme, AppID `896660` | UDP `N` et `N+1` | Ctrl+C |
| Palworld | SteamCMD anonyme, AppID `2394010` | UDP 8211 | arrêt contrôlé |
| Steam personnalisé | dépôt anonyme natif | déclaré par profil | stdin ou signal déclaré |

Les ports sont réservés en SQLite puis vérifiés contre l’hôte avant démarrage. DmxServerManager ne modifie jamais le pare-feu; ouvrez uniquement les ports des instances réellement publiées.

Un profil intégré est immuable. Une instance reste liée à une révision précise; une montée de version est explicite. Minecraft ne change jamais automatiquement de version majeure ou de loader.

## Hytale

L’installation utilise exclusivement le [downloader officiel Hytale](https://support.hytale.com/hc/en-us/articles/45326769420827-Hytale-Server-Manual). Le job passe à `waiting_for_user` pendant l’authentification device et publie uniquement l’URL officielle et le code utilisateur à durée courte. Les jetons persistants sont chiffrés dans le magasin de secrets; le fichier temporaire du downloader est supprimé avant la fin du job.

Le driver impose Java 25, lance `HytaleServer.jar` depuis `game/Server/` avec `../Assets.zip` et `HytaleServer.aot`, et n’exécute aucun script fourni par le serveur. Une sortie documentée avec le code `8` adopte uniquement un arbre complet validé depuis `game/updater/staging`. La bascule conserve mondes, configuration et mods, maintient un rollback persistant, puis confirme la mise à jour après 30 secondes stables. Un crash pendant cette fenêtre restaure automatiquement la version précédente sans perdre les changements de monde intervenus pendant l’essai.

## Minecraft Java

Chaque instance fixe une version exacte de Minecraft. Fabric, Forge, NeoForge, Quilt et Purpur exigent aussi `loader_version`; cette valeur désigne respectivement une version exacte du loader ou un numéro de build Purpur. Les alias flottants tels que `latest`, `recommended` et `stable` sont refusés. Spigot n’expose pas ce champ : la version de BuildTools est une constante de maintenance du panneau. Changer de version ou de loader nécessite une nouvelle installation; le lancement refuse un marqueur d’installation qui ne correspond plus à la configuration.

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

Le profil prend en charge les archives officielles AMD64 Linux et Windows. L’EULA doit être acceptée explicitement à la création et cette décision est auditée. En l’absence de source administrateur épinglée, le même job attend qu’un Owner fournisse l’archive officielle et son SHA-256 ; un nouvel import générique ne peut pas contourner ce contrôle.

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

## Steam personnalisé

Le profil accepte un AppID numérique, une branche publique optionnelle, un exécutable relatif, un tableau d’arguments typés, les ports et les chemins de sauvegarde. Chemins absolus, shell, URL arbitraire, exécutable hôte et compte Steam privé sont refusés. L’installation échoue clairement si le dépôt anonyme, l’OS ou AMD64 ne sont pas compatibles.

## Mods

L’onglet est affiché uniquement si le profil le déclare : Modrinth/import manuel et CurseForge optionnel pour les loaders Minecraft compatibles; plugins pour Paper/Purpur/Spigot; import manuel Hytale. Pour Bedrock, les packs restent gérés manuellement via le gestionnaire de fichiers sécurisé tant qu’aucun workflow dédié complet n’est exposé. Valheim, Palworld, Bedrock et Steam générique masquent donc l’onglet Mods par défaut.
