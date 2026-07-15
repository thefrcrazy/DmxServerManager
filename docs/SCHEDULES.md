# Tâches planifiées

Les tâches sont persistées dans SQLite et exécutées par un ordonnanceur mono-hôte. Une tâche est toujours liée à une instance et exige la permission `schedule.manage` ainsi que la permission de l'action (`server.start`, `server.stop`, `server.backup`, `server.update_game` ou `server.console.write`). Ces droits et l'état du compte sont revérifiés à chaque exécution.

Deux déclencheurs sont acceptés :

- `cron` : expression à six ou sept champs, secondes incluses, et fuseau IANA explicite, par exemple `{"kind":"cron","expression":"0 0 4 * * *","timezone":"Europe/Paris"}` ;
- `interval` : durée entière comprise entre 60 et 31 536 000 secondes, par exemple `{"kind":"interval","seconds":3600}`.

Après une interruption, une seule occurrence manquée est rattrapée. L'occurrence est identifiée par le couple tâche/date UTC, ce qui empêche les doublons au redémarrage et lors des changements d'heure. L'intervalle conserve sa phase initiale ; un cron est recalculé dans son fuseau IANA.

Les seules actions autorisées sont `start`, `stop`, `restart`, `backup`, `update` et `console`. `console` contient une commande unique sans retour à la ligne. Aucun script, shell ou chemin d'exécutable n'est accepté. En 1.0, `update` utilise le même pipeline atomique que l'action REST `install` : première installation si nécessaire, sinon mise à jour en staging avec rollback.

`GET /api/v1/schedules/{id}` renvoie un `ETag`. Le remplacement par `PUT` exige sa valeur dans `If-Match`, afin de ne pas écraser une modification concurrente.
