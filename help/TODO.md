# ServerDetails


## Joueurs


### Gestion des Joueurs
OK maintenant dans "Gestion des Joueurs" ajoute une autre tabs concernant les joueurs en DB.
- Tab whitelist lier à whitelist.json 
- Tab operateur lier à permissions.json (operateur renomme le permissions)
- Tab bannis lier à bans.json


#### whitelist.json
```
{
  "enabled": true,
  "list": [
    "550e8400-e29b-41d4-a716-446655440000",
    "123e4567-e89b-12d3-a456-426614174000",
    "abcdef12-3456-7890-abcd-ef1234567890"
  ]
}
```

#### bans.json
```
{
  "bans": [
    {
      "uuid": "550e8400-e29b-41d4-a716-446655440000",
      "username": "BadPlayer1",
      "reason": "Griefing",
      "bannedBy": "AdminName",
      "date": "2026-02-04T10:30:00Z",
      "expires": null
    },
    {
      "uuid": "123e4567-e89b-12d3-a456-426614174000",
      "username": "BadPlayer2",
      "reason": "Spam and harassment",
      "bannedBy": "ModeratorName",
      "date": "2026-02-03T15:45:00Z",
      "expires": "2026-02-10T15:45:00Z"
    }
  ]
}
```

#### permissions.json default
```
{
    "users": {},
    "groups": {
        "Default": [],
        "OP": ["*"]
    }
}
```
#### permissions.json avancé
```
{
  "groups": {
    "admin": {
      "permissions": ["*"]
    },
    "moderator": {
      "permissions": [
        "hytale.command.kick",
        "hytale.command.ban",
        "hytale.command.unban",
        "hytale.command.tp",
        "hytale.command.who",
        "hytale.accesscontrolmodule.command.ban"
      ]
    },
    "player": {
      "permissions": [
        "hytale.command.home",
        "hytale.command.sethome",
        "hytale.command.spawn"
      ]
    }
  },
  "users": {
    "06d8af17-a640-4cad-8b37-08e58820faab": {
      "groups": ["admin"]
    },
    "550e8400-e29b-41d4-a716-446655440000": {
      "groups": ["moderator"]
    },
    "123e4567-e89b-12d3-a456-426614174000": {
      "groups": ["player"]
    }
  }
}
```



[ Notes importantes ]
- Les changements manuels nécessitent un redémarrage du serveur pour prendre effet
- Les UUIDs des joueurs peuvent être obtenus via /uuid en jeu ou dans les logs du serveur
- Ameliore l'interface pour que ce soit plus simple de gérer les joueurs via selection / button (envoie de commande au serveur)






# ServerDetails


## Config

Rendre le config.json modifiable depuis l'interface
```
{
  "AuthCredentialStore": {
    "Path": "auth.enc",
    "Type": "Encrypted"
  },
  "ConnectionTimeouts": {
    "JoinTimeouts": {}
  },
  "Defaults": {
    "GameMode": "Adventure",
    "World": "default"
  },
  "DisplayTmpTagsInStrings": false,
  "LogLevels": {},
  "MOTD": "",
  "MaxPlayers": 20,
  "MaxViewRadius": 12,
  "Mods": {},
  "Modules": {
    "PathPlugin": {
      "Modules": {}
    }
  },
  "Password": "",
  "PlayerStorage": {
    "Type": "Hytale"
  },
  "Port": 5521,
  "RateLimit": {},
  "ServerName": "EntrePotes",
  "Version": 3
}
```


universe/worlds/default/config.json
```
{
  "Version": 4,
  "UUID": {
    "$binary": "BVRPpUIiSA6ZC3J3UwCUEQ==",
    "$type": "04"
  },
  "Seed": 1769517490868,
  "WorldGen": {
    "Type": "Hytale",
    "Name": "Default"
  },
  "WorldMap": {
    "Type": "WorldGen"
  },
  "ChunkStorage": {
    "Type": "Hytale"
  },
  "ChunkConfig": {},
  "IsTicking": true,
  "IsBlockTicking": true,
  "IsPvpEnabled": false,
  "IsFallDamageEnabled": true,
  "IsGameTimePaused": false,
  "GameTime": "0001-01-06T09:32:46.571824799Z",
  "ClientEffects": {
    "SunHeightPercent": 100.0,
    "SunAngleDegrees": 0.0,
    "BloomIntensity": 0.30000001192092896,
    "BloomPower": 8.0,
    "SunIntensity": 0.25,
    "SunshaftIntensity": 0.30000001192092896,
    "SunshaftScaleFactor": 4.0
  },
  "RequiredPlugins": {},
  "IsSpawningNPC": true,
  "IsSpawnMarkersEnabled": true,
  "IsAllNPCFrozen": false,
  "GameplayConfig": "Default",
  "IsCompassUpdating": true,
  "IsSavingPlayers": true,
  "IsSavingChunks": true,
  "SaveNewChunks": true,
  "IsUnloadingChunks": true,
  "IsObjectiveMarkersEnabled": true,
  "DeleteOnUniverseStart": false,
  "DeleteOnRemove": false,
  "ResourceStorage": {
    "Type": "Hytale"
  },
  "Plugin": {}
}
```


[ Notes importantes ]
- Les changements doit ce faire uniquement quand le serveur est éteint ou faire en sorte que si je save alors quand le serveur redemare cela est autosave
- Je tes mit des screenshot pour t'aider à comprendre la hierarchie