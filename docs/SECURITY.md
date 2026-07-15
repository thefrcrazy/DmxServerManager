# Sécurité

## Modèle de confiance

DmxServerManager 1.0 est un produit mono-hôte pour opérateurs de confiance. Les processus de jeux partagent le compte système du panneau. Un utilisateur autorisé à la console, aux fichiers ou aux mods peut modifier profondément une instance; n’accordez pas ces droits à une personne non fiable.

## Garanties attendues

- sessions opaques hachées en SQLite, révocables côté serveur;
- cookies `HttpOnly`, `Secure`, `SameSite=Strict` et CSRF sur toute mutation;
- Argon2id, limitation des connexions et invalidation après changement de mot de passe;
- RBAC vérifié à chaque requête avec affectation explicite des instances;
- aucune commande shell construite depuis l’API;
- chemins de fichiers confinés, sans suivi de liens symboliques;
- imports ZIP limités et contrôlés contre traversal, liens et bombes de décompression;
- webhooks limités aux hôtes HTTPS Discord officiels, sans redirection inter-hôte;
- journal d’audit des actions sensibles;
- écoute distante refusée sans TLS ou reverse proxy déclaré.

Une release native émet le cookie `Secure` même sur son listener loopback; ouvrez le panneau avec `http://localhost:5500` ou placez-le derrière HTTPS. L’unique exception sans `Secure` exige `DMX_DEV_ORIGIN`, une origine HTTP exacte sur `localhost`, `127.0.0.1` ou `::1`; elle est incompatible avec une écoute distante et avec le mode reverse proxy.

## Clé maître

La clé maître ne doit se trouver ni dans SQLite, ni dans les logs, ni dans une sauvegarde d’instance. Sur Linux natif : `/etc/dmx-server-manager/master.key`, propriétaire `root`, groupe du service, mode `0640`. Sur Windows, seuls Administrateurs, SYSTEM et le SID du service y accèdent en lecture; le service ne peut modifier ni la clé ni la configuration. Sur Docker, le bind mount `/run/secrets/dmx_master_key` conserve le propriétaire `10001:10001` et le mode `0400`. N’utilisez jamais `0444` pour contourner un problème de permissions.

Conservez une copie hors ligne chiffrée. Une rotation nécessite de rechiffrer transactionnellement tous les secrets; ne remplacez jamais simplement le fichier.

## Reverse proxy

Terminez TLS 1.2+ au proxy, activez HSTS et ne transmettez le trafic qu’à `127.0.0.1:5500`. Déclarez seulement les IP littérales du proxy dans `DMX_TRUSTED_PROXIES`. N’exposez pas le port interne du panneau en parallèle.

## Signalement

Ne publiez pas de secret ou d’exploit dans une issue. Utilisez l’onglet Security du dépôt GitHub pour ouvrir un avis privé, avec version, plateforme, impact et reproduction minimale.
