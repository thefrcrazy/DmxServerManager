# Configuration

La priorité est : variables `DMX_`, fichier TOML, valeurs sûres par défaut. Une option inconnue ou invalide bloque le démarrage au lieu d’être ignorée silencieusement.

## Variables principales

| Variable | Défaut natif | Rôle |
|---|---|---|
| `DMX_CONFIG_FILE` | `/etc/dmx-server-manager/config.toml` | fichier TOML |
| `DMX_DATA_DIR` | `/var/lib/dmx-server-manager` | instances, jobs, sauvegardes et outils |
| `DMX_DATABASE_URL` | SQLite sous le répertoire de données | base mono-hôte |
| `DMX_BIND` | `127.0.0.1:5500` | adresse HTTP interne |
| `DMX_MASTER_KEY_FILE` | clé native protégée | clé XChaCha20-Poly1305, jamais une valeur en clair |
| `DMX_STEAMCMD_PATH` | `/usr/games/steamcmd` ou toolchain Windows gérée | chemin absolu de SteamCMD; jamais configurable via l’API |
| `DMX_STATIC_DIR` | assets de la release courante | répertoire Vite contenant `index.html` et `assets/` |
| `DMX_IMPORT_ROOTS` | vide | racines autorisées pour le mode attach Owner |
| `DMX_REVERSE_PROXY` | `false` | déclare un reverse proxy TLS frontal |
| `DMX_TRUSTED_PROXIES` | vide | liste d’adresses IP littérales de proxies fiables |
| `DMX_LOG` | `info` | filtre de journalisation |
| `DMX_DEPLOYMENT_MODE` | `native` | procédure affichée : `native` ou `docker` |
| `DMX_SESSION_TTL_HOURS` | `24` | durée d’une session, entre 1 et 720 heures |
| `DMX_SETUP_TOKEN` | vide | secret temporaire requis pour créer le premier Owner hors loopback |
| `DMX_RELEASE_MANIFEST_URL` | URL officielle GitHub | override HTTPS finissant par `/release-manifest.json` |
| `DMX_RELEASE_PUBLIC_KEY` | clé publique officielle intégrée | override Ed25519 de 32 octets, encodé en base64url sans padding |
| `DMX_RELEASE_CHECK_INTERVAL_SECONDS` | `21600` | fréquence de vérification, entre 900 et 86400 secondes |
| `DMX_BEDROCK_LINUX_URL` | vide | URL HTTPS exacte de l’archive Bedrock Linux officielle |
| `DMX_BEDROCK_LINUX_VERSION` | vide | version exacte correspondant à l’archive Linux |
| `DMX_BEDROCK_LINUX_SHA256` | vide | SHA-256 vérifié par l’administrateur pour cette archive |
| `DMX_BEDROCK_LINUX_SIZE_BYTES` | vide | taille exacte optionnelle de l’archive Linux |
| `DMX_BEDROCK_WINDOWS_URL` | vide | URL HTTPS exacte de l’archive Bedrock Windows officielle |
| `DMX_BEDROCK_WINDOWS_VERSION` | vide | version exacte correspondant à l’archive Windows |
| `DMX_BEDROCK_WINDOWS_SHA256` | vide | SHA-256 vérifié par l’administrateur pour cette archive |
| `DMX_BEDROCK_WINDOWS_SIZE_BYTES` | vide | taille exacte optionnelle de l’archive Windows |

`DMX_TRUSTED_PROXIES` n’accepte pas de plage implicite. N’ajoutez que l’adresse réellement utilisée par le proxy. Les headers `Forwarded`/`X-Forwarded-*` sont ignorés pour toute autre source.

`DMX_SETUP_TOKEN` n’est utile que tant qu’aucun compte n’existe. Générez une valeur aléatoire d’au moins 32 octets, ne la journalisez pas, puis retirez-la et redémarrez le service immédiatement après la création du premier Owner. En TOML, les clés équivalentes sont `setup_token` et `session_ttl_hours`.

## Exemple Linux

```toml
bind = "127.0.0.1:5500"
data_dir = "/var/lib/dmx-server-manager"
database_url = "sqlite:///var/lib/dmx-server-manager/dmx-server-manager.sqlite?mode=rwc"
master_key_file = "/etc/dmx-server-manager/master.key"
steamcmd_path = "/usr/games/steamcmd"
static_dir = "/usr/lib/dmx-server-manager/current/static"
reverse_proxy = true
trusted_proxies = ["127.0.0.1", "::1"]
import_roots = ["/srv/importable-games"]
log = "info"
deployment_mode = "native"
session_ttl_hours = 24

# Temporaire et nécessaire seulement pour un setup initial distant.
# setup_token = "secret-aleatoire-a-retirer-apres-creation-du-owner"

# Valeurs officielles intégrées au binaire ; remplacez les deux ensemble uniquement.
# release_manifest_url = "https://github.com/thefrcrazy/DmxServerManager/releases/latest/download/release-manifest.json"
# release_public_key = "b8fhZWmHeC2hXYirmouwawPjiN3H7GMXrVixcmivg-M"
# release_check_interval_seconds = 21600
```

## Détection de release signée

Les builds officiels utilisent l’URL GitHub et la clé publique Ed25519 suivie dans `backend/release-public-key.b64url` lorsque les deux options sont absentes. Une configuration env ou TOML remplace ce défaut uniquement si elle fournit la paire complète : une seule valeur, une URL HTTP, un hôte non officiel, un port explicite, une query ou une clé de mauvaise taille bloque le démarrage. La clé est publique : ne la placez pas dans le fichier de secrets. La clé privée correspondante reste exclusivement dans l’environnement de signature de la release.

Le backend télécharge une enveloppe JSON bornée, suit au maximum trois redirections HTTPS vers les hôtes GitHub autorisés, vérifie strictement la signature Ed25519 du payload brut, puis exige :

- une version SemVer et des notes GitHub correspondant exactement au tag ;
- un SHA-256 pour chaque archive et chaque installateur Linux/Windows ;
- l’image `ghcr.io/thefrcrazy/dmx-server-manager` avec un digest `sha256:` exact ;
- les trois cibles AMD64, même si l’installation courante n’en utilise qu’une.

Le panneau vérifie au démarrage puis selon l’intervalle configuré. L’Owner peut relancer une vérification. Aucune commande n’est exécutée et le processus ne se remplace jamais lui-même.

## Sources Minecraft Bedrock

Microsoft ne publie pas de manifeste Bedrock stable fournissant à la fois version, URL et checksum. DmxServerManager n’invente donc aucun endpoint ni digest. Deux flux explicites sont disponibles :

- configurer une source épinglée avec URL officielle, version et SHA-256 ; les trois valeurs sont obligatoires et toute redirection reste limitée aux hôtes HTTPS officiels autorisés ;
- laisser la source vide : le job d’installation passe à `waiting_for_user`. L’Owner télécharge l’archive depuis la [page officielle Bedrock](https://www.minecraft.net/en-us/download/server/bedrock), calcule son SHA-256, puis envoie le ZIP sur la route indiquée par le job avec le header `x-dmx-archive-sha256`.

Configuration TOML équivalente pour une source épinglée :

```toml
[bedrock_linux_source]
url = "https://www.minecraft.net/bedrockdedicatedserver/bin-linux/bedrock-server-VERSION.zip"
version = "VERSION"
sha256 = "64-caracteres-hexadecimaux"
# size_bytes = 0 # facultatif ; si présent, utilisez la taille réelle non nulle
```

Ne recopiez pas les placeholders ci-dessus. Le démarrage refuse une source partielle, HTTP, un hôte non officiel, une version flottante, un digest absent/mal formé ou une archive supérieure à 4 Gio. La version est une décision explicite : aucune montée majeure automatique n’est effectuée.

## Exposition réseau

Le processus refuse une adresse non-loopback si aucun certificat TLS n’est configuré et si `reverse_proxy` est faux. Ne contournez pas cette protection. En développement, gardez loopback et utilisez le proxy Vite local.

## Secrets

La clé maître doit contenir exactement 32 octets aléatoires. Sur Docker, elle est montée dans `/run/secrets/dmx_master_key`; elle n’est jamais passée dans l’environnement. Sauvegardez cette clé séparément de `/data`, avec accès restreint. Sa perte rend les secrets chiffrés irrécupérables.

Les clés CurseForge, mots de passe de jeux et URLs Discord sont configurés via l’API protégée. Les réponses exposent uniquement `configured: true|false`.
