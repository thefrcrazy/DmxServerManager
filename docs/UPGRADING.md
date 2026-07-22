# Mise à niveau et rollback

Le panneau détecte un manifeste de release signé mais ne se remplace jamais pendant son exécution. Lisez les notes de compatibilité et créez une sauvegarde avant toute montée de version.

## Linux natif

```bash
sudo DMX_VERSION=1.1.6 \
  DMX_EXPECTED_ARCHIVE_SHA256='<checksum-archive-du-manifeste-signé>' \
  sh ./dmx-server-manager-install-linux.sh
```

Utilisez l’asset d’installateur dont le SHA-256 est inclus dans le manifeste Ed25519 vérifié par le panneau, ou vérifiez son bundle Sigstore comme dans le guide d’installation initiale. L’installateur conserve chaque archive dans un répertoire immuable `<version>-<sha256>` sous `/usr/lib/dmx-server-manager/releases`, bascule atomiquement le lien `current`, puis exige un healthcheck HTTP réussi. La commande proposée par le panneau transmet le checksum signé via `DMX_EXPECTED_ARCHIVE_SHA256`; l’installateur ne consulte jamais un fichier `.sha256` distant de sa propre initiative. Toute erreur après l’arrêt du service restaure le lien, l’unité systemd et l’état actif précédents. Pour un rollback manuel, listez d’abord les digests disponibles :

```bash
sudo systemctl stop dmx-server-manager
ls -1 /usr/lib/dmx-server-manager/releases
sudo ln -sfn /usr/lib/dmx-server-manager/releases/1.1.6-<sha256> /usr/lib/dmx-server-manager/current
sudo systemctl start dmx-server-manager
```

Ne rétrogradez pas après une migration de schéma déclarée irréversible; restaurez alors la sauvegarde créée avant mise à niveau.

## Windows natif

Relancez la commande proposée par le panneau, qui appelle `install.ps1 -Version 1.1.6 -ExpectedArchiveSha256 <checksum-signé>`. Lorsque ce paramètre est présent, aucun fichier `.sha256` distant n’est consulté. Chaque archive reste sous `%ProgramFiles%\DmxServerManager\releases\<version>-<sha256>` et `current` est une jonction vers la release active. L’installateur restaure la jonction, la configuration SCM, l’environnement et l’état antérieur si le service ou son healthcheck HTTP échoue.

## Docker

```bash
cd /opt/dmx-server-manager
docker compose pull panel
docker compose up -d --force-recreate panel
```

`docker compose pull` récupère `ghcr.io/thefrcrazy/dmx-server-manager:latest`; `up --force-recreate` remplace ensuite le conteneur en conservant `config/` et `data/`. Un `docker pull` seul ne modifie jamais un conteneur déjà lancé.

La commande proposée par le panneau utilise le digest signé exact. Pour un rollback, remplacez `DMX_IMAGE` dans `.env` par un tag immuable tel que `ghcr.io/thefrcrazy/dmx-server-manager:1.0.7`, puis relancez les deux commandes. Ne supprimez jamais `config/master.key` ni `data/`.

## Format du manifeste du panneau

La source configurée sert une enveloppe JSON stricte :

```json
{
  "payload": "<JSON UTF-8 encodé en base64url sans padding>",
  "signature": "<signature Ed25519 de 64 octets en base64url sans padding>"
}
```

Le payload signé utilise `schema_version: 1` et contient `version`, `published_at`, `notes_url`, les cibles `native.linux_amd64` et `native.windows_amd64` avec URL et SHA-256 de l’archive et de l’installateur, ainsi que `docker.image` et `docker.digest`. Les champs supplémentaires sont refusés. Une signature valide sans checksum complet reste invalide.

La clé privée de signature n’est jamais installée sur le panneau ni placée dans le dépôt. La clé publique officielle est suivie dans `backend/release-public-key.b64url` et intégrée au binaire ; `DMX_RELEASE_PUBLIC_KEY` permet seulement un override administré, fourni avec l’URL correspondante. Une rotation de clé exige une nouvelle version authentifiée par l’ancienne chaîne de confiance ou une opération manuelle explicite ; n’acceptez jamais une nouvelle clé annoncée par le manifeste qu’elle est censée authentifier.

Le workflow de publication exige le secret `DMX_RELEASE_SIGNING_KEY_PEM` dans l’environnement GitHub Actions protégé `release`, contenant une clé privée Ed25519 PEM. L’absence du secret, une autre famille de clé ou une clé ne correspondant pas exactement à `backend/release-public-key.b64url` bloque la publication avant la création de la draft. Le tag doit pointer vers un commit déjà présent sur `main`. L’Owner approuve explicitement l’accès à l’environnement pour la création de la draft, puis une seconde fois pour sa publication ; un tag seul ne peut donc jamais lire la clé sans validation manuelle. Seuls ces deux jobs accèdent à l’environnement ; les jobs de compilation n’obtiennent jamais la clé. L’enveloppe produite est ensuite décodée et sa signature revérifiée localement avant upload.

Une publication reste en draft jusqu’à la réussite des archives natives, du conteneur, des scans, des signatures et du manifeste. En cas d’échec, le job de rollback ne supprime une version GHCR que si la release est encore une draft, que son digest correspond exactement à celui construit par ce run et qu’elle est sans tag ou ne porte que le tag de version attendu ; toute ambiguïté bloque la suppression. La draft non publiée est ensuite supprimée même si le nettoyage GHCR a échoué, afin de permettre une reprise contrôlée. Conservez alors les logs, vérifiez manuellement le digest et les tags dans GHCR, puis supprimez uniquement la version non publiée correspondante avant de relancer le tag. Ne déplacez jamais un tag Git ou un tag d’image déjà publié.

L’administrateur de release dérive une fois la valeur publique, la vérifie par un canal distinct, puis la fige dans `backend/release-public-key.b64url` avant toute publication :

```bash
openssl pkey -in release-signing-key.pem -pubout -outform DER \
  | tail -c 32 | base64 | tr '+/' '-_' | tr -d '=\n'
```

Ne transmettez jamais le fichier PEM privé aux installations. Le panneau rejette une version signée strictement inférieure à sa propre version et n’affiche une procédure que pour une version strictement supérieure.

Chaque tag `v*` publie les archives Linux/Windows, les installateurs natifs, leurs SHA-256 utiles, des SBOM SPDX et des bundles Sigstore keyless. Vérifiez le checksum d’archive et l’installateur avant la première installation :

```bash
cosign verify-blob \
  --bundle dmx-server-manager-v1.1.6-x86_64-unknown-linux-gnu.tar.gz.sha256.sigstore.json \
  --certificate-identity https://github.com/thefrcrazy/DmxServerManager/.github/workflows/release.yml@refs/tags/v1.1.6 \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  dmx-server-manager-v1.1.6-x86_64-unknown-linux-gnu.tar.gz.sha256
cosign verify-blob \
  --bundle dmx-server-manager-install-linux.sh.sigstore.json \
  --certificate-identity https://github.com/thefrcrazy/DmxServerManager/.github/workflows/release.yml@refs/tags/v1.1.6 \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  dmx-server-manager-install-linux.sh
```

Le bootstrap Docker exécute cette vérification `cosign verify` avec l’issuer GitHub Actions et l’identité exacte du workflow `release.yml` pour le tag demandé. Ne contournez pas le bootstrap et ne déployez jamais un tag flottant ou une image dont le digest/signature ne correspond pas à la release.
