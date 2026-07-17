# Exploitation et dépannage

## Commandes de service

```bash
sudo systemctl status dmx-server-manager
sudo journalctl -u dmx-server-manager -f
sudo systemctl restart dmx-server-manager
```

```powershell
Get-Service DmxServerManager
Restart-Service DmxServerManager
Get-WinEvent -LogName Application | Where-Object ProviderName -eq 'DmxServerManager'
```

```bash
cd /opt/dmx-server-manager
docker compose ps
docker compose logs -f panel
```

## Sauvegardes

Une sauvegarde est un job avec progression, archive streaming et SHA-256. Le driver gèle proprement le jeu ou l’arrête. Les binaires, caches, logs et secrets sont exclus. Une restauration crée d’abord une sauvegarde de sécurité, restaure en staging, valide puis bascule atomiquement avec rollback.

Ne copiez pas une base SQLite active avec `cp`. Utilisez l’API de sauvegarde du panneau ou arrêtez le service. Sauvegardez la clé maître séparément : elle ne fait volontairement pas partie des archives.

## Incidents courants

### Le conteneur ne démarre pas

Vérifiez que `config/master.key` appartient à `10001:10001` en mode `0400`, que `config/config.toml` est lisible par le groupe `10001` et que `data/` est accessible à l’UID 10001. Consultez `docker compose logs panel`.

### Le panneau refuse l’écoute distante

C’est une protection attendue. Revenez à `127.0.0.1:5500` ou configurez un reverse proxy HTTPS et ses IP dans `DMX_TRUSTED_PROXIES`.

### Une instance ne démarre pas

Consultez le Job et son identifiant de trace. Vérifiez l’espace disque, le conflit réel TCP/UDP, la disponibilité du dépôt anonyme Steam, l’architecture AMD64, l’EULA et la version Java demandée. Ne remplacez pas l’exécutable ou les arguments via une valeur API non prévue.

### Une installation semble bloquée

Ouvrez le Job puis **Voir le terminal d’installation**. Le panneau relit jusqu’à 10 000 lignes persistantes de stdout/stderr, continue avec le flux SSE en direct et permet de copier toutes les lignes visibles. Pour Hytale, le terminal conserve les messages et erreurs réels du downloader et ajoute des diagnostics DMX sur la phase, les arguments non sensibles, le PID, la durée, le nombre de lignes, le nombre de demandes OAuth, l’état du fichier de credentials, le layout extrait (`Assets.zip`, JAR et cache AOT optionnel) et la classification d’échec. Le code, les paramètres OAuth éphémères et les jetons restent masqués. La carte sécurisée conserve la route exacte du downloader `https://oauth.accounts.hytale.com/oauth2/device/verify`, sa casse de code et les paramètres de la session. Vérifiez le code, puis approuvez aussi l’écran des permissions. Si la demande expire et que le downloader quitte, relancez l’installation afin de créer une nouvelle session ; ne réutilisez jamais un ancien code.

Le terminal d’installation reste volontairement en lecture seule : Hytale utilise son flux OAuth device, Minecraft exige l’EULA dans la configuration, SteamCMD est anonyme et les autres installateurs sont non interactifs. Aucune saisie shell arbitraire n’est transmise à un installateur.

Bedrock est téléchargé automatiquement depuis le lien stable publié par le service public utilisé par `minecraft.net`. Si ce service officiel est indisponible, le job peut encore proposer l’import manuel sécurisé comme procédure de secours.

Les messages Palworld `SteamAPI ... before SteamAPI_Init succeeded` peuvent apparaître pendant l’initialisation Unreal. Le critère de disponibilité est la ligne `Running Palworld dedicated server on :PORT`. Une sortie `error code: 130` lors d’un arrêt demandé correspond à l’interruption gracieuse du processus et n’indique pas à elle seule une installation corrompue.

En dernier recours, les journaux bornés se trouvent dans `instances/<id>/logs/` sous le dossier de données. `install.combined.log` conserve l’ordre combiné de stdout/stderr ; les messages techniques restent intacts et seuls les codes OAuth, paramètres d’autorisation et secrets sont masqués, jamais écrits en clair.

### Docker Desktop Windows

Vérifiez que le réseau hôte est activé, que les conteneurs Linux sont utilisés et qu’Enhanced Container Isolation est désactivé. Préférez un volume Docker à un bind mount NTFS.
