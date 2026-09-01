# Audit complet — DmxServerManager v1.1.6

**Date :** 31 août 2026
**Périmètre :** frontend React (24 928 lignes TS/TSX + 9 676 lignes SCSS), backend Rust (52 288 lignes), chaîne de build, tests et QA.
**Axe principal demandé :** qualité UI/UX en laptop, tablette et mobile.

---

## 1. Résumé exécutif

L'application est **techniquement saine mais visuellement et ergonomiquement inachevée hors desktop**. Le backend est solide (headers de sécurité complets, 300 tests, échappement ANSI sûr), le lint et le typecheck passent sans avertissement, les 55 tests unitaires frontend passent. Le problème n'est pas la qualité du code métier : c'est que **la couche présentation n'a jamais été conçue pour autre chose qu'un écran large**, et que rien dans la chaîne de tests ne pouvait le détecter.

Trois constats structurants :

1. **Il n'existe pas de palier tablette.** Un seul point de bascule réel (`1023px`) sépare « pile mono-colonne mobile » de « layout desktop complet ». Entre 768 px et 1023 px l'application affiche la mise en page mobile étirée sur 1000 px de large ; à 1024 px exactement elle affiche le layout desktop dans 792 px utiles, ce qui écrase les cartes.
2. **Le mobile est fonctionnel mais pas utilisable.** Rien ne déborde horizontalement (le test existant le vérifie), mais les écrans sont saturés de blocs décoratifs : sur `Console`, il reste **74 px sur 844** pour le terminal. Sur le journal d'audit mobile, les colonnes « action » et « cible » sont masquées en CSS — le journal indique *qui* et *quand*, jamais *quoi*.
3. **La QA de design existante (`design-qa.md`) conclut « aucun problème P0/P1/P2 » alors que ses 9 captures de preuve sont toutes en 1680×1080 ou 1542×1080.** Le responsive y est déclaré vérifié sans une seule capture tablette ou mobile. Le seul rendu mobile versionné est `docs/visual-references/v1.1.0/08-dashboard-mobile.png`, un unique écran.

**Un défaut fonctionnel bloquant** a par ailleurs été trouvé hors du périmètre UI : **l'éditeur de configuration avancé (Monaco) ne peut pas fonctionner en production** (§4.1).

### Compteurs bruts mesurés

| Viewport | Cibles tactiles < 44 px | Champs < 16 px | Textes < 12 px | Éléments hors viewport |
|---|---:|---:|---:|---:|
| laptop 1366×768 | 81 | 22 | 10 | 0 |
| laptop 1280×800 | 73 | 22 | 10 | 0 |
| tablette paysage 1024×768 | 72 | 22 | 10 | 1 |
| tablette portrait 834×1112 | 70 | 22 | 9 | 0 |
| tablette portrait 768×1024 | 68 | 22 | 9 | 1 |
| mobile 390×844 | 72 | 22 | 9 | 14 |
| mobile 360×740 | 72 | 22 | 9 | 17 |

**163 nœuds** en violation de contraste WCAG AA, sur **64 combinaisons vue × viewport**.

---

## 2. Méthode

Un harnais Playwright temporaire a été écrit pour instrumenter l'application réelle via la fixture `ApiMock` existante (`frontend/e2e/api.fixture.ts`), puis retiré du dépôt à la fin de l'audit. Il a mesuré, **16 routes × 7 viewports = 112 rendus** :

- largeur de document vs viewport (débordement horizontal) ;
- boîtes englobantes de tous les éléments interactifs visibles (cibles tactiles) ;
- `font-size` calculée de tous les champs de saisie et de tous les nœuds texte feuilles ;
- conteneurs à débordement horizontal interne (`scrollWidth > clientWidth`) ;
- éléments dont le bord droit dépasse le viewport ;
- analyse axe-core (`wcag2a`, `wcag2aa`, `wcag21a`, `wcag21aa`, `wcag22aa`).

**49 captures d'écran** ont été produites et conservées dans `artifacts/ui-audit/` (laptop 1366, tablette 768, mobile 390), avec le relevé brut dans `artifacts/ui-audit/probe.json`.

Le code a ensuite été relu manuellement : primitives UI, contextes, hooks, feuilles SCSS, pipeline de build, backend Rust (sécurité, panics, en-têtes HTTP).

Routes couvertes : `dashboard`, `servers`, `activity?tab=operations`, `activity?tab=journal`, `user-settings`, `administration`, `servers/create`, et l'instance serveur sur ses onglets `overview`, `config`, `console`, `files`, `backups`, `metrics`, `players`, `schedules`, `mods`.

---

## 3. Verdict par plateforme

| Plateforme | Note | Synthèse |
|---|---|---|
| **Laptop (1280–1440)** | 6/10 | Fonctionnel et lisible. Troncatures visibles sur la carte « Connexion », hiérarchie d'actions inversée (le bouton destructeur est le plus visible), densité d'information faible. |
| **Tablette (768–1024)** | 3/10 | **Le maillon le plus faible.** Aucun palier dédié. Cartes pleine largeur sur 700 px pour afficher 3 mots. Onglets d'administration coupés sans indice de défilement dès 768 px et jusqu'à 1024 px inclus. |
| **Mobile (360–430)** | 4/10 | Pas de débordement, mais l'écran utile est confisqué par des blocs décoratifs. Zoom automatique iOS sur tous les champs. Journal d'audit amputé. Cibles tactiles jusqu'à 17 px de haut. |

---

## 4. Anomalies P0 — bloquantes

### 4.1 L'éditeur Monaco charge depuis un CDN externe que la CSP de production bloque

**Constat.** `@monaco-editor/react` est utilisé dans `frontend/src/components/features/server/NativeConfigEditorModal.tsx:1`, mais `loader.config()` n'est jamais appelé. Le loader utilise donc son chemin par défaut : `https://cdn.jsdelivr.net/npm/monaco-editor@0.55.1/min/vs`.

**Preuve (interception réseau réelle, éditeur ouvert) :**

```
script     https://cdn.jsdelivr.net/npm/monaco-editor@0.55.1/min/vs/loader.js
script     https://cdn.jsdelivr.net/npm/monaco-editor@0.55.1/min/vs/editor/editor.main.js
script     https://cdn.jsdelivr.net/npm/monaco-editor@0.55.1/min/vs/nls.messages-loader.js
script     https://cdn.jsdelivr.net/npm/monaco-editor@0.55.1/min/vs/basic-languages/monaco.contribution.js
stylesheet https://cdn.jsdelivr.net/npm/monaco-editor@0.55.1/min/vs/editor/editor.main.css
other      https://cdn.jsdelivr.net/npm/monaco-editor@0.55.1/min/vs/assets/editor.worker-Be8ye1pW.js
… 12 ressources au total
```

La CSP servie par le backend (`backend/src/main.rs:327`) est :

```
default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self'
```

`cdn.jsdelivr.net` n'y figure pas. **L'éditeur avancé est donc inopérant sur toute installation réelle**, et a fortiori sur un déploiement LAN ou hors ligne — la cible affichée du produit.

**Facteurs aggravants :**
- `monaco-editor@0.56.0` est déclaré en dépendance directe (`frontend/package.json`) mais **n'est importé nulle part** — poids mort, et version différente de celle réellement chargée (0.55.1).
- Les tests e2e passent parce que Playwright cible le **serveur de développement Vite**, qui n'émet aucune CSP. Les en-têtes de sécurité n'existent que côté backend Rust. Aucun test ne s'exécute donc dans les conditions de production.
- Le test `e2e/auth-dashboard.spec.ts:305` et `e2e/visual-references.spec.ts:87` valident tous deux `.monaco-editor` visible — et donnent donc une fausse assurance.

**Correctif.** Empaqueter Monaco localement :

```ts
// main.tsx, avant tout rendu
import * as monaco from "monaco-editor";
import { loader } from "@monaco-editor/react";
loader.config({ monaco });
```

puis configurer les workers Monaco dans `vite.config.ts`. Ajouter un test e2e exécuté **contre le binaire backend** (la config `playwright.real.config.ts` existe déjà, elle ne contient que 2 tests) qui ouvre l'éditeur et échoue sur toute requête sortante hors origine.

---

### 4.2 Le journal d'audit mobile masque l'action et la cible

**Constat.** `frontend/src/styles/pages/_operations.scss:234-245` :

```scss
@media (max-width: 800px) {
    .audit-row { grid-template-columns: 8rem minmax(0, 1fr) auto; }
    .audit-row > :nth-child(3),
    .audit-row > :nth-child(4) { display: none; }   // action + cible
    .operation-row progress,
    .operation-row time { display: none; }          // progression + horodatage
}
```

**Preuve.** `artifacts/ui-audit/mobile-390--activity-journal.png` : la seule ligne affichée est `13/07/2026 14:00:00 | owner | Réussi`. L'événement `server.updated` visible en desktop a disparu.

**Impact.** Sur un panel dont `docs/SECURITY.md` fait un argument produit, le journal d'audit consulté depuis un téléphone n'indique plus **quelle action a été réalisée ni sur quelle ressource**. Ce n'est pas une adaptation responsive, c'est une perte de données. Idem pour les opérations : la progression d'un job et son horodatage sont masqués sur mobile, alors que « suivre une installation depuis son téléphone » est précisément le cas d'usage mobile principal de ce produit.

**Correctif.** Passer les lignes en présentation empilée (`display: block` + libellés en `::before` ou en `<dl>`) plutôt que masquer des colonnes. Aucune information d'audit ne doit dépendre de la largeur de l'écran.

---

### 4.3 Zoom automatique iOS sur 100 % des champs de saisie

**Constat.** Tous les champs sont en **13 px** :
- `frontend/src/styles/components/_form.scss:16` — `.input { font-size: 0.8125rem }`
- `frontend/src/styles/_mixins.scss` (`@mixin input-base`) — `font-size: 0.8125rem`
- `.form-input` et `.select` héritent via `@extend .input`

Safari iOS déclenche un zoom automatique irréversible dès qu'un champ de moins de 16 px reçoit le focus. L'utilisateur doit ensuite dézoomer à la main, et la barre supérieure fixe sort du cadre.

**Preuve (22 champs distincts mesurés, extrait) :**

```
input#setting-port.input            13px   (7 vues)
input#instance-name.input           13px   (6 vues)
input#secret-server_password.input  13px   (6 vues)
input#current-password.input        13px
input#new-password.input            13px
input#managed-user-password.input   13px
select#managed-user-role.form-input 13px
input.console-input                 13px
```

**Impact.** Connexion, changement de mot de passe obligatoire, création d'instance, configuration de profil, envoi de commande console, création de compte : **tous** les parcours de saisie mobiles sont affectés.

**Correctif.** Un seul point à modifier :

```scss
.input, .form-input, .select, textarea {
    font-size: var(--font-size-base);          // 14px, conservé desktop
    @media (max-width: 1023px) { font-size: 1rem; }  // 16px, supprime le zoom iOS
}
```

---

## 5. Anomalies P1 — majeures

### 5.1 Aucun garde-fou de contraste sur la couleur d'accentuation

**Constat.** La couleur d'accentuation est libre côté client (`<input type="color">`, `frontend/src/components/ui/ColorPicker.tsx:38`) et le backend n'en valide que le format :

```rust
// backend/src/api/administration.rs:774
fn validate_accent_color(value: &str) -> Result<(), AppError> {
    if value.len() == 7 && value.starts_with('#')
        && value[1..].bytes().all(|b| b.is_ascii_hexdigit()) { Ok(()) }
    else { Err(AppError::BadRequest("users.invalid_accent_color".into())) }
}
```

Or cette couleur sert de **couleur de texte** pour les onglets actifs, les liens et les intitulés de section.

**Preuve (axe, accent `#4f46e5` — valeur réelle d'un compte de test) :**

```
#server-detail-tab-configuration   #4f46e5 sur #000000   ratio 3.33:1  (requis 4.5:1)
#server-detail-tab-console         #4f46e5 sur #000000   ratio 3.33:1
.profile-config-overview__eyebrow  #4f46e5 sur #111111   ratio 3.00:1  (12px gras)
```

**L'incohérence est interne au projet :** le backend valide déjà le contraste pour les thèmes du catalogue — `backend/src/services/catalog.rs:1330` rejette un thème si `contrast_ratio(foreground, bg_primary) < 3.0`. La même règle n'est pas appliquée à l'accent utilisateur. Et le seuil retenu (3.0) correspond au texte large / composants d'interface, alors que l'accent est utilisé sur du texte de 12 à 13 px, qui exige **4.5**.

**Correctif.** Réutiliser `contrast_ratio` dans `validate_accent_color` avec un seuil de 4.5 contre `bg_primary`, et refléter la contrainte dans `ColorPicker` (désactiver ou corriger automatiquement la teinte hors seuil). Relever le seuil catalogue de 3.0 à 4.5 pour les jetons utilisés en texte.

---

### 5.2 `.btn--danger` : blanc sur rouge à 3.76:1, et redéfini globalement par une feuille de page

**Constat.** `_button.scss:43` définit une variante danger **discrète et conforme** (fond rouge 10 %, texte rouge). Mais `frontend/src/styles/pages/_server-detail.scss:1090` — au niveau racine, donc **globalement** — la remplace :

```scss
.btn--danger {
    background-color: var(--color-danger);   // #EF4444
    color: white;
    border: none;
}
```

`main.scss` important `pages/server-detail` après `components/button`, cette règle gagne **partout dans l'application**, y compris dans les boîtes de dialogue de confirmation et le tiroir Activité.

**Preuve axe :** `fgColor #ffffff, bgColor #ef4444, contrastRatio 3.76, fontSize 13px, expected 4.5:1` — 42 occurrences relevées.

**Impact double :** échec WCAG AA sur le bouton le plus critique de l'application (suppression d'instance, arrêt forcé, révocation de sessions) **et** rupture de l'encapsulation du design system : une feuille de page pilote un composant global.

**Correctif.** Supprimer la redéfinition de `_server-detail.scss`, et utiliser la variante `--danger-solid` existante (`_button.scss:53`) qui utilise déjà `color: #000000` avec un commentaire documentant son ratio de 5.58:1.

---

### 5.3 Le mobile perd son écran utile au profit de blocs décoratifs

**Preuve.** `artifacts/ui-audit/mobile-390--srv-console.png` : sur 844 px de hauteur, l'onglet Terminal commence à **y ≈ 770**. Deux lignes de log visibles.

Décomposition mesurée sur `mobile-390` :

| Bloc | Hauteur | Information portée |
|---|---:|---|
| Barre supérieure fixe | 64 px | titre + sous-titre (2 lignes) |
| 4 cartes `StatPill` empilées | ≈ 425 px | « En ligne », « 1 », « 0.219.16 », un port masqué |
| Rangée d'actions (2 lignes) | ≈ 90 px | 4 boutons |
| Onglets (2 lignes) | ≈ 100 px | 4 onglets |
| **Reste pour le contenu** | **≈ 74 px** | — |

Le même schéma se répète sur `dashboard` (4 cartes pour « 1, 0, 0, 0 » — `artifacts/ui-audit/mobile-390--dashboard.png`) et sur `servers`, où le bandeau « Ressources système » consomme 470 px avant la première instance.

**Cause CSS :**
- `_server-detail.scss:41-49` — `.server-header-stats { grid-template-columns: 1fr }` sous `@include mobile`
- `_dashboard.scss:135` — `.dashboard-header-stats { grid-template-columns: 1fr }` sous `max-width: 640px`

**Correctif.** Passer ces grilles en 2 colonnes compactes jusqu'à 360 px (`repeat(2, minmax(0,1fr))` + suppression de l'icône décorative de 40 px sous 640 px). Gain estimé : ~250 px sur les deux écrans. Rendre le bandeau de ressources hôte repliable sur mobile ou le déplacer dans le tableau de bord uniquement.

---

### 5.4 Onglets d'administration coupés sans indice de défilement, du mobile à la tablette paysage

**Constat.** `frontend/src/styles/pages/_administration.scss:33` — `.administration-tabs { overflow-x: auto }`, sans dégradé de bord, sans flèches, sans `scroll-snap`, et sans `scrollIntoView` sur l'onglet actif. Sur macOS la barre de défilement est invisible par défaut : rien n'indique qu'il y a davantage d'onglets.

**Mesures (largeur intrinsèque de la barre : 776 px) :**

| Viewport | Largeur disponible | Onglets hors champ |
|---|---:|---|
| mobile 390 | 358 px | Catalogue, Fournisseurs de mods, Webhooks, Mise à jour |
| tablette 768 | 736 px | Mise à jour du panneau |
| tablette 1024 | 743 px | Mise à jour du panneau |

**Preuve.** `artifacts/ui-audit/mobile-390--administration.png` — 4 onglets sur 8 visibles, « Profils Steam » sur deux lignes tandis que les autres tiennent sur une, ce qui casse l'alignement de la rangée.

**Impact.** Sur tablette et mobile, des sections entières d'administration sont **inaccessibles à un utilisateur qui ne devine pas qu'il faut faire glisser la barre**. La mise à jour du panneau est notamment invisible jusqu'à 1024 px inclus.

**Correctif.** Sous 1024 px, basculer sur un `<select>` de navigation ou une grille d'onglets sur deux lignes ; à défaut, ajouter un masque de dégradé aux deux bords et `scroll-snap-type: x mandatory`.

---

### 5.5 Barre supérieure fixe à 64 px, mais son contenu monte à 78 px

**Constat.** `_topbar.scss:12` — `height: var(--header-height)` (64 px), sans `min-height` ni adaptation mobile. Le sous-titre injecté par `PageTitleContext` n'est jamais tronqué.

**Mesures :**

| Viewport | Route | Hauteur contenu | Lignes sous-titre | Dépassement |
|---|---|---:|---:|---:|
| 360×740 | `/servers/create` | **78 px** | 4 | **+7 px** |
| 390×844 | `/servers/create` | 64 px | 3 | 0 px (limite) |
| 360/390 | dashboard, servers, administration, activité, compte | 49 px | 2 | — |
| ≥ 768 | toutes | 35–36 px | 1 | — |

**Preuve.** `artifacts/ui-audit/mobile-390--create-server.png` — le texte de la barre touche le contenu de la page.

**Bug connexe :** le bouton retour n'a pas de `flex: 0 0 auto` et se fait comprimer par le flex parent :

```
a.topbar__back-btn   32×32 px  (desktop, tablette)
a.topbar__back-btn   29×32 px  (mobile 390, /servers)
a.topbar__back-btn   25×32 px  (mobile 360, /servers)
a.topbar__back-btn   22×32 px  (mobile 360/390, /servers/create)
```

**22 px de large pour le bouton « retour » sur un téléphone.**

**Correctif.** `flex: 0 0 auto` + `min-width: 44px; min-height: 44px` sur `.topbar__back-btn` et `.topbar__mobile-menu` ; masquer le sous-titre sous 768 px (ou le tronquer à une ligne avec `-webkit-line-clamp: 1`) ; passer `--header-height` à `auto` avec `min-height: 64px`.

---

### 5.6 Toasts totalement invisibles aux lecteurs d'écran

**Constat.** `frontend/src/components/shared/ToastContainer.tsx` et `Toast.tsx` ne comportent **ni `role="status"`, ni `role="alert"`, ni `aria-live`**. Le bouton de fermeture (`Toast.tsx:19`) n'a **pas d'`aria-label`**.

**Impact.** `toast.error(...)` est le canal d'erreur principal de l'application — utilisé dans `ServerDetail`, `Administration`, `CreateServer`, tous les clients API. Un utilisateur de lecteur d'écran ne reçoit **aucun retour** sur le succès ou l'échec de ses actions.

**Aggravation.** Auto-suppression à 5 s (`ToastContext.tsx:39`) sans pause au survol ni au focus, et sans distinction entre succès et erreur — un message d'erreur disparaît avant d'avoir pu être lu ou atteint au clavier (WCAG 2.2.1).

**Correctif.** `role="status" aria-live="polite"` sur le conteneur, `role="alert" aria-live="assertive"` pour le type `error`, `aria-label` sur la fermeture, et pas d'auto-suppression pour les erreurs.

---

### 5.7 Dialogues modaux sans piège de focus ni restauration

**Constat.** `frontend/src/components/shared/DialogContainer.tsx` :
- `aria-modal="true"` déclaré, mais **le fond n'est ni `inert` ni `aria-hidden`** ;
- **aucun piège de focus** : `Tab` sort du dialogue et parcourt la page en arrière-plan ;
- **aucune restauration de focus** à la fermeture — le focus retombe sur `<body>` ;
- `setTimeout(..., 100)` (ligne 15) pour le focus du champ, contre `requestAnimationFrame` (ligne 17) dans l'autre branche — deux stratégies incohérentes, la première étant temporellement fragile ;
- **`inputRef` est attaché à deux champs distincts** (lignes 81 et 94) : si un dialogue `prompt` portait aussi une `verificationString`, la seconde référence écraserait la première ;
- chaîne française en dur non traduite ligne 91 : `"Veuillez saisir le texte de confirmation"` — **seule chaîne visible non internationalisée de l'application avec `Select.tsx:27`** ;
- l'overlay est un `<div onClick>` sans rôle ni équivalent clavier ; un `onClick` (plutôt qu'un `onMouseDown` avec test de cible) ferme le dialogue si une sélection de texte se termine sur l'overlay.

Le tiroir Activité (`Activity.tsx:257`) présente les mêmes manques : `role="dialog" aria-modal="true"` sans déplacement de focus à l'ouverture, sans piège, sans restauration. Il gère en revanche correctement `Escape` (lignes 166-173).

---

### 5.8 `100vh` sur mobile, aucune zone de sécurité

**Constat.** 8 usages de `100vh`, 1 seul `100dvh` (`_server-detail.scss:3143`) :

```
_base.scss:31          body      min-height: 100vh
_base.scss:50          #root     min-height: 100vh
_layout.scss:6         .layout   min-height: 100vh
_layout.scss:22        .main-content  height: 100vh
_sidebar.scss:5        .sidebar  height: 100vh
_feedback.scss:106     overlay   height: 100vh
_form.scss:155         .modal    max-height: 85vh
_login.scss:8, 68      min-height: 100vh
```

Sur iOS Safari et Android Chrome, `100vh` correspond au viewport **barre d'URL masquée**. `.main-content` étant en `height: 100vh; overflow: hidden` avec le défilement délégué à `.page-container`, le bas de chaque page passe sous la barre du navigateur tant que celle-ci n'est pas repliée.

**Zéro usage de `env(safe-area-inset-*)`** dans tout le projet : sur iPhone à encoche en paysage, la barre supérieure fixe et le tiroir latéral passent sous les coins arrondis.

**`index.html` ne déclare ni `viewport-fit=cover`, ni `theme-color`** — la barre système du navigateur reste claire au-dessus d'une application entièrement noire.

**Correctif.** `100dvh` partout, `viewport-fit=cover`, `padding: env(safe-area-inset-*)` sur la barre supérieure, le tiroir et le pied des modales, `<meta name="theme-color" content="#090909">`.

---

## 6. Anomalies P2 — importantes

### 6.1 Cibles tactiles : 109 tailles distinctes sous 44 px sur mobile

Les plus critiques (largeur × hauteur en px) :

| Taille | Élément | Écran |
|---|---|---|
| **13×13** | case à cocher native | création de serveur |
| **16×16** | case à cocher native | config, administration |
| **26×26** | `Afficher l'adresse` (œil) | liste des serveurs |
| **22×32** | `topbar__back-btn` | création de serveur |
| **34×22** | `Actualiser` (`btn--sm`) | activité |
| **294×17** | `Révoquer les autres` (`btn--sm`) | mon compte |
| **228×19** | `Diagnostics internes` (`<summary>`) | détail instance |
| **36×36** | `Ouvrir la navigation` (menu hamburger) | toutes |
| **34×34** | `Envoyer` (console), `Redémarrer` | console, serveurs |

**Cause racine.** `_button.scss:75` — `.btn--sm { padding: 0.125rem var(--spacing-2); font-size: 0.6875rem }` : 2 px de rembourrage vertical et 11 px de police donnent **17 px de hauteur totale**. `@mixin button-base` produit ≈ 34 px. `.btn--icon` est figé à 34 px (`_button.scss:88`). `_form.scss:213` fixe les cases à cocher à 16×16.

Le menu hamburger — **le seul point d'entrée vers la navigation sur mobile** — mesure 36×36.

**Correctif.** Sous 1024 px, appliquer `min-height: 44px; min-width: 44px` à `.btn`, `.btn--icon`, `.topbar__mobile-menu`, `.topbar__back-btn` ; agrandir les cases à cocher à 20 px avec une zone cliquable étendue via `::before`. Supprimer `.btn--sm` sur tactile ou lui donner le même gabarit que `.btn`.

### 6.2 Carte « Connexion » tronquée dès 1366 px

**Preuve.** `artifacts/ui-audit/laptop-1366--srv-overview.png` : le titre affiche « CONNEXI… », la valeur « ••••••… », l'aide « Utilisez la … ».

**Mesures :**

```
mobile 390          small   contenu 337px dans 144px  (−193px)
tablette 1024       small   contenu 337px dans  39px  (−298px)
tablette 1024       span.connection-pill__value  86px dans 39px
```

**Cause.** `.server-header-stats` utilise `repeat(auto-fit, minmax(200px, 1fr))` (`_server-detail.scss:43`) : les quatre cartes reçoivent une largeur identique alors que « Connexion » porte un libellé, une valeur monospace, **deux boutons d'action** et une ligne d'aide. Cette aide est de surcroît en `font-size: 10px; white-space: nowrap; text-overflow: ellipsis` (`_server-detail.scss:70`) — sans attribut `title`, le texte complet est **définitivement inaccessible**.

**Correctif.** `grid-column: span 2` sur `.connection-pill` au-dessus de 900 px ; autoriser le retour à la ligne de l'aide (`white-space: normal`) ; supprimer le `font-size: 10px` au profit de `--font-size-xs`.

### 6.3 Hiérarchie d'actions inversée : le bouton destructeur est le plus visible

Sur toute la page de détail d'instance (laptop, tablette et mobile), **« Forcer l'arrêt » est le seul bouton plein**, en rouge saturé, tandis que « Redémarrer » et « Arrêter » sont en contour et « Voir l'activité » en fantôme gris (lu comme désactivé).

L'action la plus dangereuse — un `SIGKILL` sur un serveur de jeu, avec perte de sauvegarde possible — est celle vers laquelle l'œil est attiré en premier. Sur mobile, elle est en outre adjacente à « Arrêter » sur la même ligne (`artifacts/ui-audit/mobile-390--srv-overview.png`), ce qui crée un risque réel d'appui erroné, aggravé par des cibles de 33 à 35 px.

**Correctif.** `--danger` en contour discret pour « Forcer l'arrêt », reléguer cette action derrière un menu « … » ou dans la zone de danger, et donner l'accent plein à l'action primaire contextuelle (Démarrer / Redémarrer).

### 6.4 Tableau des joueurs : défilement horizontal inaccessible au clavier

**Violation axe `scrollable-region-focusable`** (2 occurrences, `mobile-390` et `mobile-360`, onglet Joueurs).

```
div.table-scroll.server-players__table   contenu 453px dans 282px
table, thead, tr, th, td, time           bord droit à 503px (viewport 390px)
```

Le conteneur défile mais n'est pas atteignable au clavier (`tabIndex` absent) : un utilisateur clavier ne peut pas voir les colonnes de droite. Le même défaut existe sur `.console-output` (`ServerConsole.tsx:169`), qui n'a ni `tabIndex={0}`, ni `role`, ni `aria-label`, ni `aria-live`.

### 6.5 Trois implémentations d'onglets divergentes

| Implémentation | Fichier | Roving tabindex | Flèches | `aria-controls` | `role="tabpanel"` |
|---|---|:-:|:-:|:-:|:-:|
| `Tabs` (détail instance) | `components/ui/Tabs.tsx` | ✅ | ✅ | ✅ | ✅ |
| Administration | `pages/Administration.tsx:143-190` | ✅ | ✅ (dupliqué) | ✅ | ✅ |
| Activité | `pages/Activity.tsx:214-219` | ❌ | ❌ | ❌ | ❌ |

Les onglets d'Activité sont de simples `<button role="tab">` : les trois sont dans l'ordre de tabulation (le motif ARIA en exige un seul), les flèches ne font rien, et **aucun panneau n'a `role="tabpanel"`**. La logique clavier d'Administration duplique ≈ 25 lignes déjà présentes dans `Tabs.tsx`.

**Correctif.** Faire converger les trois sur `components/ui/Tabs.tsx`, en lui ajoutant une variante « défilante » qui appelle `scrollIntoView` sur l'onglet actif.

### 6.6 Info-bulles inutilisables au clavier, au toucher et au lecteur d'écran

`frontend/src/components/ui/Tooltip.tsx` :
- déclenchement **uniquement** sur `onMouseEnter`/`onMouseLeave` — pas de `onFocus`/`onBlur`, pas de gestion tactile ;
- pas de `role="tooltip"` ni de `aria-describedby` — le contenu n'existe pas pour les technologies d'assistance ;
- pas de fermeture par `Escape` (WCAG 1.4.13 exige un contenu au survol *dismissible*) ;
- **le repositionnement écoute `window.scroll`** (ligne 72) alors que le défilement de cette application a lieu dans `.page-container`, `.console-output` et `.table-scroll` — l'info-bulle **reste figée à des coordonnées obsolètes** et flotte au-dessus d'un contenu sans rapport dès qu'on fait défiler la page ;
- aucun recadrage sur les bords du viewport ;
- les enfants sont enveloppés dans un `<div>` qui perturbe les contextes flex environnants.

### 6.7 `ServerCard` : rôle interactif contenant des éléments interactifs

`frontend/src/components/features/server/ServerCard.tsx:48-60` applique `role="link"` + `tabIndex={0}` sur un `<div>` qui contient plusieurs `<button>` d'action. L'imbrication d'éléments interactifs dans un élément à rôle interactif rompt la navigation au lecteur d'écran, et `role="link"` activé par `Espace` diverge du comportement natif d'un lien.

### 6.8 Sélecteur de profil dupliqué à la création de serveur

`frontend/src/pages/CreateServer.tsx:349-361` rend un `<select id="server-profile" className="sr-only">` **et** un `role="radiogroup"` de boutons (ligne 360), tous deux câblés sur `selectProfile`. Un lecteur d'écran annonce donc **deux fois** la même liste d'options, et le `<label htmlFor="server-profile">` visible étiquette le `<select>` caché, pas le groupe radio visible (qui utilise un `aria-label` distinct).

De plus, les `role="radio"` n'ont **ni roving tabindex ni navigation par flèches** — le motif ARIA `radiogroup` n'est pas respecté.

*(Le probe a détecté cet élément sous la forme `select#server-profile.sr-only` de 1×1 px avec un contenu de 102 px.)*

### 6.9 Sur tablette, le layout desktop est activé dans un espace insuffisant

Le point de bascule unique est **1023 px** (`_layout.scss:59`, `_sidebar.scss:116`, `_topbar.scss:110`). Un iPad en paysage (1024, 1080 ou 1180 px de large) reçoit donc la barre latérale fixe de 232 px, ne laissant que **792 px** de contenu — moins qu'un iPad en portrait, qui bénéficie de la pleine largeur.

C'est ce qui produit l'écrasement mesuré à 1024 px :

```
div.stat-pill__content       contenu  78px dans 39px
span.stat-pill__label        contenu  78px dans 39px
span.connection-pill__value  contenu  86px dans 39px
small                        contenu 337px dans 39px
```

**Correctif.** Introduire un palier tablette : barre latérale rétractée en icônes (`--sidebar-collapsed-width: 60px`, **déjà déclarée dans `_variables.scss:82` et jamais utilisée**) entre 1024 et 1279 px, barre complète au-delà.

---

## 7. Dette du design system

### 7.1 La police principale n'est jamais chargée

`_variables.scss:62` déclare `--font-family-sans: "Inter", …`. **Aucun `@font-face`, aucun fichier de police, aucun lien Google Fonts** n'existe dans le projet. Inter n'est donc jamais appliquée : l'application retombe sur `ui-sans-serif` / `system-ui`, et le rendu diffère entre macOS, Windows et Android.

Idem pour `_base.scss:117` : `code, pre { font-family: "JetBrains Mono", "Fira Code", monospace }` — deux polices absentes, et qui **contournent le jeton `--font-family-mono`** pourtant correctement défini avec des solutions de repli système.

La CSP (`script-src 'self'`, pas de `font-src`) impose de toute façon un auto-hébergement.

### 7.2 194 classes CSS mortes sur 511 (38 %)

Classes déclarées en SCSS et jamais référencées dans le TS/TSX/HTML :

| Fichier | Classes mortes |
|---|---:|
| `pages/_server-detail.scss` | **140** |
| `components/_server-list.scss` | 16 |
| `pages/_servers.scss` | 14 |
| `pages/_setup.scss` | 9 |
| `components/_topbar.scss` | 5 |
| `pages/_create-server.scss` | 5 |
| `components/_select.scss` | 4 |
| autres (11 fichiers) | 11 |

Vérifications ponctuelles : `.file-manager`, `.file-modal`, `.context-menu`, `.editor-toolbar`, `.backup-list`, `.config-action-bar`, `.col-cpu`, `.btn-kill`, `.checkbox-component`, `.custom-select__option` → **0 usage**. Ce sont les vestiges d'un gestionnaire de fichiers et d'un sélecteur personnalisé remplacés depuis.

`_server-detail.scss` fait **3 147 lignes** dont 63 lignes de code commenté et 140 classes inutilisées, pour une page de 615 lignes de TSX.

### 7.3 34 classes déclarées dans plusieurs fichiers

`pages/_servers.scss` duplique quasi intégralement `components/_server-list.scss` : `.server-row`, `.server-name`, `.server-icon`, `.server-link`, `.usage-bar`, `.text-cell`, `.col-*`, `.btn-kill`, `.server-actions`, `.text-right`… avec des valeurs légèrement divergentes.

C'est la cause directe des **33 `!important`** du projet :

```
components/_server-list.scss:38   .server-name  { display: flex !important; }
components/_server-list.scss:95   display: flex !important;
pages/_server-detail.scss:2240    background-color: #DC2626 !important;
pages/_server-detail.scss:2241    color: #FFFFFF !important;
```

`components/_server-list.scss:598-606` **réimplémente `.sr-only` inline** avec 7 `!important` alors que la classe existe déjà dans `_base.scss:37`.

### 7.4 Jetons de conception contournés

**Couleurs.** 138 valeurs `rgba()` en dur et 61 valeurs hexadécimales hors `_variables.scss`, réparties dans 11 fichiers. Puisque les thèmes sont entièrement personnalisables (`ThemeContext` applique 14 jetons à l'exécution, y compris des fonds clairs), **un thème personnalisé casse toute cette couche**. Exemple emblématique : `_topbar.scss:13` — `background-color: rgba(16, 16, 16, 0.92)` : la barre supérieure reste noire quel que soit le thème.

**Typographie.** L'échelle s'arrête à `--font-size-xs: 0.75rem` (12 px), mais 12 déclarations sont en `10px`/`11px` en dur et 12 autres en `0.625rem`/`0.6875rem` :

```
_server-detail.scss:70, 1456, 2524, 2538, 2644, 2665, 2682, 2835, 2844, 2937, 3062   →  10px
_server-detail.scss:2320                                                              →  11px
_metrics.scss:114                                                                     →  0.625rem
_button.scss:77, _metrics.scss:68/121/141, _server-list.scss:137,
_server-tools.scss:96/368, _topbar.scss:69, _operations.scss:81                        →  0.6875rem
```

**Points de rupture.** Onze valeurs différentes, aucune adossée aux jetons `--breakpoint-*` : `480, 640, 680, 700, 768, 800, 900, 980, 1000, 1023, 1100`. Les mixins `respond-to` et `mobile` (`_mixins.scss:113-144`) ne sont utilisés que 12 fois sur 34 media queries.

### 7.5 Trois valeurs par défaut concurrentes pour la couleur d'accentuation

| Source | Valeur |
|---|---|
| `frontend/src/styles/_variables.scss:6` | `#4f8cff` |
| `frontend/src/constants/theme.ts:18` | `#3A82F6` |
| `backend/migrations/0001_dmx_server_manager.sql:19` | `#3A82F6` |

La valeur CSS s'applique avant que `ThemeProvider` n'injecte les jetons — un **flash de couleur visible à chaque chargement**.

### 7.6 Deux composants pour chaque rôle

- **Sélection :** `components/ui/Select.tsx` (nommé « custom-select » mais rendant un `<select>` natif) coexiste avec des `<select className="input">` bruts (`Activity.tsx:232`, `CreateServer.tsx`, `UserManagement.tsx`). Deux apparences dans la même application.
- **Choix de couleur :** `ColorPicker` avec pastilles et coche sur « Mon compte », `<input type="color">` brut dans l'administration (`input#managed-user-accent`, 48×32 px).
- **Champs :** `.input` et `.form-input`, ce dernier n'étant qu'un `@extend .input` avec le commentaire « alias for compatibility » (`_form.scss:80`). Les deux noms circulent dans le code.

`@extend` est par ailleurs employé deux fois (`.form-input`, `.select`) : en Sass il duplique la liste de sélecteurs à chaque occurrence de `.input`, ce qui gonfle le CSS livré (**171 KB non compressés**, 24,8 KB gzip).

### 7.7 Code mort dans les composants

- **`headerActions` : fonctionnalité entièrement morte.** Le champ existe dans `PageTitleContext.tsx:12,22,33`, est rendu dans `Layout.tsx:129-133`, est stylé par `.topbar__actions` (`_topbar.scss:75`) et `.header-actions-group` (`_topbar.scss:270`)… mais **aucun des 7 appels à `setPageTitle` ne passe ce quatrième argument**. La règle `@media (max-width: 768px) { .header-actions-group { display: none } }` (`_topbar.scss:351`) masque donc du vide.
- `SafeAnsi` déclare une prop `useClasses` jamais lue (`SafeAnsi.tsx:83`), que `ServerConsole.tsx:200` passe pourtant explicitement.
- `--sidebar-collapsed-width: 60px` (`_variables.scss:82`) n'est jamais utilisée.

### 7.8 Densité et lisibilité du JSX

Certains rendus sont écrits sur une seule ligne :

| Fichier | Ligne la plus longue |
|---|---:|
| `administration/SteamProfileManagement.tsx` | **899 caractères** |
| `pages/ServerDetail.tsx` | 692 caractères |
| `pages/Activity.tsx` | 640 caractères |

18 lignes dépassent 200 caractères dans `Activity.tsx`, 29 dans `SteamProfileManagement.tsx`. Le rendu complet du tableau d'audit et du tiroir d'Activité tient sur 5 lignes.

---

## 8. Accessibilité — synthèse

### Violations axe confirmées

| Règle | Gravité | Nœuds | Vues |
|---|---|---:|---:|
| `color-contrast` | serious | 163 | 64 |
| `scrollable-region-focusable` | serious | 2 | 2 |

Cibles distinctes du contraste :

```
42×  .server-actions > .btn--danger.btn      #ffffff / #ef4444   3.76:1
42×  #server-detail-tab-configuration        #4f46e5 / #000000   3.33:1
36×  .profile-config-overview__eyebrow       #4f46e5 / #111111   3.00:1
22×  .btn--danger                            #ffffff / #ef4444   3.76:1
 7×  #server-detail-tab-console              #4f46e5 / #000000   3.33:1
 7×  #server-detail-tab-players              #4f46e5 / #000000   3.33:1
 7×  #server-detail-tab-schedules            #4f46e5 / #000000   3.33:1
```

### Écarts non détectables par axe

| Écart | Emplacement | Critère |
|---|---|---|
| Toasts sans `aria-live` | `ToastContainer.tsx` | 4.1.3 |
| Bouton de fermeture de toast sans nom | `Toast.tsx:19` | 4.1.2 |
| Aucun piège de focus dans les modales | `DialogContainer.tsx`, `Activity.tsx:257` | 2.4.3 |
| Aucune restauration de focus | idem | 2.4.3 |
| Info-bulles sans `focus`, sans `Escape`, sans `aria-describedby` | `Tooltip.tsx` | 1.4.13 / 2.1.1 |
| Onglets Activité sans flèches ni `tabpanel` | `Activity.tsx:214` | 2.1.1 / 4.1.2 |
| `radiogroup` sans roving tabindex | `CreateServer.tsx:360` | 2.1.1 |
| Interactif imbriqué dans `role="link"` | `ServerCard.tsx:48` | 4.1.2 |
| Erreurs de champ sans `aria-invalid`/`aria-describedby` | `components/ui/Input.tsx:16` | 3.3.1 |
| `.console-output` non focalisable | `ServerConsole.tsx:169` | 2.1.1 |
| `prefers-reduced-motion` ignoré | 2 sélecteurs couverts sur 17 `animation:` + 66 `transition:` + `scroll-behavior: smooth` | 2.3.3 |
| Tableau d'audit sans sémantique de table | `Activity.tsx:250` (`div.audit-row` + `span`) | 1.3.1 |

### Couverture axe actuelle

`e2e/accessibility-i18n.spec.ts` n'analyse que **2 écrans sur 16** (connexion et tableau de bord), en Desktop Chrome uniquement. Les écrans les plus riches — détail d'instance, administration, création de serveur — ne sont jamais analysés. C'est précisément là que se trouvent les 163 nœuds en défaut.

### Points positifs

`html[lang]` est bien synchronisé avec la langue active (vérifié par test), la structure `main` / `section` / `header` est correcte, les icônes décoratives portent `aria-hidden`, le tiroir mobile gère `Escape` et le focus initial, `Tabs.tsx` implémente correctement le roving tabindex, et `ColorPicker` expose `aria-pressed` avec une coche visuelle.

---

## 9. Performance

### 9.1 Console : rendu non virtualisé de 10 000 lignes

`ServerConsole.tsx:189-211` rend l'intégralité de `logs` dans le DOM. `useServerEvents.ts:14` autorise `MAX_VISIBLE_INSTALL_LOG_LINES = 10_000`, et le README annonce explicitement ce rejeu.

Chaque ligne produit un `<div>` plus un `<span>` par segment ANSI (`SafeAnsi.tsx:89`), soit **plus de 10 000 nœuds**, et chaque ligne subit **4 appels `String.includes`** recalculés à chaque rendu (lignes 191-194).

Or `useServerEvents.ts:100-103` déclenche un rendu **par ligne reçue** :

```ts
setLogs((current) => [...current, formatLogLine(...)].slice(-visibleLogLimit(logSource)));
```

Pendant une installation SteamCMD, qui émet des centaines de lignes par seconde, cela produit deux allocations de tableau de 10 000 éléments et un rendu complet de 10 000 nœuds **par ligne**. C'est la cause la plus probable des ralentissements perçus, et elle est rédhibitoire sur mobile.

### 9.2 `mergeLogHistory` en O(n²) à chaque reconnexion

`useServerEvents.ts:46-57` : pour `size` décroissant de `maxOverlap` à 1, la fonction exécute `history.slice(-size)` puis `.every(...)`. Avec 10 000 lignes d'historique et 10 000 lignes en direct, cela représente jusqu'à **50 millions de comparaisons et 10 000 allocations de tableau**, sur le thread principal, à chaque `stream.reset`, `stream.lagged` ou erreur SSE.

### 9.3 Reconnexion SSE sans temporisation

`useServerEvents.ts:152-156` :

```ts
source.onerror = () => {
    setIsConnected(false);
    void loadHistory();     // requête REST complète
    onServerUpdate();       // seconde requête REST
};
```

`EventSource` retente automatiquement toutes les ~3 s et déclenche `onerror` à chaque échec. Si le backend est indisponible, le navigateur **martèle deux endpoints REST toutes les 3 secondes indéfiniment**, sans repli exponentiel ni plafond de tentatives.

### 9.4 Logo de 368 KB affiché en 42 px

`frontend/public/dmx-server-manager-logo.png` : **1254×1254 px, 368 711 octets**. Il est affiché en `height: 42px` dans la barre latérale (`_sidebar.scss:27`), en favicon 32 px, et sur les écrans de connexion, de configuration initiale et de changement de mot de passe.

**360 KB téléchargés sur chaque chargement à froid** pour une image rendue à 42 px. C'est le plus gros actif du projet, devant `services-DLuRJ8hQ.js` (274 KB).

### 9.5 Illustrations de jeux servies par des CDN tiers

`frontend/src/constants/gameProfiles.ts:10-56` : 9 des 10 profils chargent leur illustration depuis `static-cdn.jtvnw.net` (Twitch) et `shared.akamai.steamstatic.com` (Valve), en 600×800 ou 616×353.

Cela **contredit le README**, qui annonce « illustrations locales des jeux », et pose trois problèmes :

1. **Hors ligne / LAN** — la cible affichée du produit : les images ne se chargent jamais.
2. **Confidentialité** : le navigateur de l'administrateur expose son IP à Twitch et Valve, et révèle quels jeux il héberge. (`referrerPolicy="no-referrer"` est correctement appliqué, ce qui limite la fuite au seul niveau IP.)
3. `public/game-art/` ne contient que **6 SVG** pour 10 profils : Satisfactory, 7 Days to Die, Project Zomboid et Rust n'ont **aucun repli propre** et retombent sur le logo Steam générique.

Les balises `<img>` sont correctement en `loading="lazy"` mais **sans `width`/`height`** — décalage de mise en page au chargement.

### 9.6 Pas de `Cache-Control` sur les actifs statiques

`backend/src/main.rs:267-269` sert `dist/` via `ServeDir` sans en-tête de cache. Vite produit pourtant des noms de fichiers empreintés (`index-CeS6z6Bl.js`), qui autorisent `Cache-Control: public, max-age=31536000, immutable`. En l'état, chaque chargement effectue une requête conditionnelle par actif — un aller-retour complet par fichier sur une liaison mobile.

Symétriquement, `index.html` est servi sans `Cache-Control: no-cache`, ce qui peut faire pointer une page mise en cache vers des actifs empreintés supprimés après une mise à jour.

### 9.7 Poids livré

| Actif | Brut | gzip |
|---|---:|---:|
| `services-DLuRJ8hQ.js` | 274 KB | 83 KB |
| `index-CeS6z6Bl.js` | 213 KB | 66 KB |
| `index-BZLlSX6w.css` | **171 KB** | 25 KB |
| `server-BQ2k30L0.js` | 94 KB | 23 KB |
| `Administration-JaPpJrTp.js` | 72 KB | 16 KB |

Le CSS de 171 KB pour une application de 16 écrans s'explique par les 38 % de classes mortes et le double usage de `@extend`.

---

## 10. Backend Rust — revue ciblée

Le backend est **globalement de bonne facture**. Points vérifiés :

### Points forts

- **En-têtes de sécurité complets** (`main.rs:308-340`) : `X-Content-Type-Options`, `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`, CSP stricte, `Permissions-Policy`, HSTS conditionnel derrière reverse proxy.
- **Validation PNG faite main** (`services/catalog.rs:1339-1400`) : bornes correctement vérifiées avant chaque `try_into().unwrap()`, CRC contrôlé, liste blanche de chunks, limites de dimensions. Les `unwrap()` de ce bloc sont **sûrs** après relecture.
- **Validation de contraste des thèmes du catalogue** (`services/catalog.rs:1330`) — la bonne idée, appliquée au mauvais périmètre (cf. §5.1).
- **300 tests backend**, `#[cfg(test)]` bien isolés.
- **Rendu ANSI sûr côté frontend** : `SafeAnsi` produit exclusivement des nœuds texte React, jamais de HTML ni de lien. La capture `mobile-390--srv-console.png` montre `<img src=x onerror=alert(1)>` correctement affiché en texte brut.

### Points d'attention

**`RwLock` empoisonné = panne totale du registre de profils.** `services/profiles.rs` contient 9 occurrences de `.expect("profile registry poisoned")` (lignes 54, 65, 75, 83, 93, 100, 109, 138, 197). Une seule panique survenant pendant la détention du verrou empoisonne définitivement le `RwLock` : **toutes les requêtes suivantes touchant les profils paniqueront**, jusqu'au redémarrage du service. Envisager `parking_lot::RwLock` (qui n'a pas de notion d'empoisonnement) ou une récupération explicite via `PoisonError::into_inner`.

**Pas de `CatchPanicLayer`.** Les features `tower-http` retenues sont `cors, fs, trace, limit` — sans `catch-panic`. Une panique dans un gestionnaire tue la tâche et coupe la connexion sans réponse HTTP propre.

**55 `unwrap`/`expect`/`panic!` hors modules de test**, sur 1 446 au total. La grande majorité sont des invariants légitimes (regex constantes dans `services/players.rs:429-492`, `api/auth.rs:43`). Aucun n'est atteignable par une entrée utilisateur d'après la relecture, hors le cas d'empoisonnement ci-dessus.

**`unreachable!()` dans un `match` métier** (`services/schedules.rs:631`) : à convertir en erreur typée si le jeu de variantes peut évoluer.

---

## 11. Tests et QA — l'angle mort

C'est l'explication structurelle de tout ce qui précède.

| Dimension | État |
|---|---|
| Lint ESLint | ✅ propre (`--max-warnings 0`) |
| TypeScript strict | ✅ propre |
| Tests unitaires frontend | ⚠️ 55 tests, **0 rendu de composant** |
| Tests de composants React | ❌ inexistants (pas de `@testing-library/react`) |
| Projets Playwright | ❌ **1 seul : `Desktop Chrome`** |
| WebKit / Safari | ❌ aucun — c'est pourtant là que se manifestent le zoom iOS et `100vh` |
| Firefox | ❌ aucun |
| Viewport mobile | ⚠️ 2 tests ponctuels en 390×844 |
| Viewport tablette | ❌ **aucun** |
| Analyse axe | ⚠️ 2 écrans sur 16 |
| Parité des clés FR/EN | ⚠️ non testée (vérifiée manuellement : conforme) |
| Tests e2e « pile réelle » | ⚠️ 2 tests, ne couvrent pas l'éditeur |

Les 55 tests unitaires portent tous sur les clients API, les schémas Zod et les utilitaires purs :

```
operationsClient 23 · gameProfiles 7 · catalog 6 · apiTransport 5
nativeConfigForm 4 · adminClient 3 · apiContract 3 · safeAnsi 3
releasesClient 2 · password 1
```

**Aucun composant React n'est jamais monté dans un test unitaire.** La seule vérification d'interface passe par Playwright, en Chrome desktop, contre le serveur de développement Vite — c'est-à-dire sans CSP, sans les en-têtes du backend, et sans jamais simuler un navigateur mobile.

`design-qa.md` affirme « Responsive : les grilles et outils passent en une colonne et les tables restent défilables sur petit écran » et conclut « final result: passed », alors que ses 9 captures de preuve dans `artifacts/design-qa/` sont **toutes en 1680×1080 ou 1542×1080**. La revue responsive annoncée n'a pas été instrumentée.

---

## 12. Anomalies P3 — mineures

| # | Constat | Emplacement |
|---|---|---|
| 1 | Un onglet indisponible dans l'URL réinitialise l'affichage sur « Config » **sans corriger l'URL** — un lien partagé `?tab=files` ouvre silencieusement le mauvais onglet | `ServerDetail.tsx:289-291` |
| 2 | `<Button as="link" disabled>` rend un lien pleinement cliquable — `disabled` est retiré par la déstructuration et jamais réappliqué | `components/ui/Button.tsx:60-66` |
| 3 | `{...(props as any)}` dans un projet TypeScript strict | `components/ui/Button.tsx:62` |
| 4 | `// @ts-expect-error` pour rendre une icône de composant | `components/ui/Tabs.tsx:55` |
| 5 | `Input` : la prop `error` s'affiche sans `aria-invalid` ni `aria-describedby` | `components/ui/Input.tsx:16` |
| 6 | Placeholder français en dur `"Sélectionner..."` | `components/ui/Select.tsx:27` |
| 7 | Chaîne française en dur `"Veuillez saisir le texte de confirmation"` | `DialogContainer.tsx:91` |
| 8 | `commandHistories` : `Map` de module jamais purgée, croît à chaque instance visitée | `ServerConsole.tsx:15` |
| 9 | Classification des logs par `includes("ERROR")` — un pseudo de joueur ou un chemin déclenche le style d'erreur ; les 4 classes peuvent s'appliquer simultanément | `ServerConsole.tsx:191-194` |
| 10 | `className` construit sur 5 lignes avec indentation, injectée telle quelle dans le DOM | `ServerConsole.tsx:198-203` |
| 11 | ANSI noir rendu en `#161b22` — invisible sur fond de console quasi noir ; palette ANSI insensible au thème | `SafeAnsi.tsx:8` |
| 12 | `.select { color-scheme: dark }` en dur — un thème clair conserve des menus natifs sombres | `_form.scss:91` |
| 13 | `.modal { max-height: 85vh }` — devrait être `dvh` | `_form.scss:155` |
| 14 | `.server-detail-page { max-width: 1600px }` imbriquée dans `.page-container { max-width: 1440px }` — la contrainte interne est inopérante | `_server-detail.scss:5` vs `_layout.scss:33` |
| 15 | 63 lignes de CSS commenté, dont 3 tentatives successives de `height: calc(...)` | `_server-detail.scss:6-11, 230…` |
| 16 | `clearLogs` et `clearPendingBedrockArchive` recréés à chaque rendu ; l'objet retourné par le hook change d'identité à chaque rendu | `useServerEvents.ts:194-206` |
| 17 | Largeur de barre latérale lue depuis `localStorage` dans un `useEffect` après un état initial figé à 232 — décalage visible au chargement | `Layout.tsx:22, 40-43` |
| 18 | Redimensionnement de la barre latérale sans `user-select: none` pendant le glissement (sélection de texte parasite) et sans relâchement explicite du pointeur | `Sidebar.tsx:68-72` |
| 19 | Route `*` redirigée silencieusement vers `/dashboard` — aucune page 404, une URL erronée est indiscernable d'une navigation normale | `App.tsx:102` |
| 20 | Le basculement liste/grille occupe une ligne complète sur mobile alors que les deux modes y rendent une colonne unique | `Servers.tsx` / `_server-list.scss` |
| 21 | Statut de serveur affiché **trois fois** dans la même carte (pastille sur l'illustration, point près du titre, ligne « Statut ») | `ServerCard.tsx` |
| 22 | Format i18n à plusieurs clés par ligne — nuit à la relecture et fait échouer tout contrôle de parité naïf | `i18n/fr.ts:576, 626` |
| 23 | La CSP autorise `steamstatic.com` et `jtvnw.net` en `img-src` — surface externe à supprimer une fois les illustrations rapatriées | `main.rs:327` |

---

## 13. Plan d'action proposé

### Lot 1 — Correctifs bloquants (1 à 2 jours)

1. Empaqueter Monaco localement + `loader.config({ monaco })`, retirer la dépendance inutilisée, ajouter un test e2e « pile réelle » qui échoue sur toute requête hors origine. *(§4.1)*
2. Rétablir les colonnes « action » et « cible » du journal d'audit sur mobile en présentation empilée. *(§4.2)*
3. Passer les champs de saisie à 16 px sous 1024 px. *(§4.3)*

### Lot 2 — Fondations responsive (3 à 5 jours)

4. Introduire un **palier tablette** : barre latérale en icônes de 1024 à 1279 px (la variable `--sidebar-collapsed-width` existe déjà). *(§6.9)*
5. Normaliser les points de rupture sur 4 valeurs (`640 / 1024 / 1280 / 1536`) et n'utiliser que les mixins. *(§7.4)*
6. Barre supérieure : `min-height` au lieu de `height`, sous-titre masqué sous 768 px, `flex: 0 0 auto` sur les boutons. *(§5.5)*
7. `100dvh` partout, `viewport-fit=cover`, `env(safe-area-inset-*)`, `<meta name="theme-color">`. *(§5.8)*
8. Grilles de statistiques en 2 colonnes sur mobile ; bandeau de ressources hôte repliable. *(§5.3)*
9. Onglets d'administration : `<select>` de navigation ou grille sur deux lignes sous 1024 px. *(§5.4)*
10. `min-height: 44px` sur toutes les cibles tactiles sous 1024 px ; suppression de `.btn--sm` sur tactile. *(§6.1)*

### Lot 3 — Accessibilité (2 à 3 jours)

11. Garde-fou de contraste sur l'accent, seuil 4.5, côté backend **et** dans `ColorPicker`. *(§5.1)*
12. Supprimer la redéfinition globale de `.btn--danger` dans `_server-detail.scss`. *(§5.2)*
13. `aria-live` sur les toasts, `aria-label` sur la fermeture, pas d'auto-suppression des erreurs. *(§5.6)*
14. Piège de focus + restauration dans `DialogContainer` et le tiroir Activité. *(§5.7)*
15. `Tooltip` : `focus`/`blur`, `Escape`, `aria-describedby`, écoute du défilement en phase de capture. *(§6.6)*
16. Faire converger les trois implémentations d'onglets sur `components/ui/Tabs.tsx`. *(§6.5)*
17. Bloc `@media (prefers-reduced-motion: reduce)` global. *(§8)*
18. Étendre l'analyse axe aux 16 routes, sur 3 viewports.

### Lot 4 — Dette du design system (3 à 5 jours)

19. Purger les 194 classes mortes ; scinder `_server-detail.scss` par onglet. *(§7.2)*
20. Fusionner `_servers.scss` dans `_server-list.scss` et supprimer les 33 `!important`. *(§7.3)*
21. Remplacer les 138 `rgba()` et 61 hexadécimaux en dur par des jetons ; introduire `--font-size-2xs` ou supprimer les tailles de 10–11 px. *(§7.4)*
22. Auto-héberger Inter et le jeu monospace, ou aligner les jetons sur les polices système. *(§7.1)*
23. Unifier le sélecteur, le sélecteur de couleur et les champs sur un composant unique ; supprimer l'alias `.form-input`. *(§7.6)*
24. Retirer la fonctionnalité morte `headerActions`. *(§7.7)*

### Lot 5 — Performance (2 à 3 jours)

25. Virtualiser la console ; regrouper les lignes SSE par `requestAnimationFrame` ; précalculer la classification à l'insertion. *(§9.1)*
26. Réécrire `mergeLogHistory` avec un index de hachage sur le suffixe. *(§9.2)*
27. Repli exponentiel sur `EventSource.onerror`. *(§9.3)*
28. Rapatrier les illustrations de jeux en local (WebP + SVG de repli pour les 4 profils manquants) ; retirer les deux domaines de la CSP. *(§9.5)*
29. Redimensionner le logo (128 px + favicon dédié) — **−360 KB par chargement à froid**. *(§9.4)*
30. `Cache-Control: immutable` sur les actifs empreintés, `no-cache` sur `index.html`. *(§9.6)*

### Lot 6 — Filet de sécurité QA (1 à 2 jours)

31. Ajouter les projets Playwright `Mobile Safari` (WebKit), `Mobile Chrome` et `iPad`.
32. Intégrer le harnais de mesure de cet audit en test de non-régression : cibles tactiles < 44 px, champs < 16 px, débordement horizontal, éléments hors viewport — avec seuils bloquants.
33. Introduire `@testing-library/react` et couvrir en priorité `Tabs`, `Button`, `Select`, `DialogContainer`, `Tooltip`, `ToastContainer`.
34. Test de parité des clés FR/EN.

---

## 14. Annexe — artefacts produits

```
artifacts/ui-audit/
├── probe.json                       relevé brut : 112 mesures (16 routes × 7 viewports)
├── laptop-1366--*.png               16 captures
├── tablet-port-768--*.png           16 captures
└── mobile-390--*.png                16 captures
```

Captures les plus démonstratives :

| Fichier | Ce qu'il montre |
|---|---|
| `mobile-390--srv-console.png` | 74 px utiles pour le terminal sur 844 px |
| `mobile-390--activity-journal.png` | Journal d'audit sans action ni cible |
| `mobile-390--administration.png` | 4 onglets sur 8, sans indice de défilement |
| `mobile-390--create-server.png` | Barre supérieure débordant sur le contenu |
| `laptop-1366--srv-overview.png` | Carte « Connexion » tronquée, hiérarchie d'actions inversée |
| `tablet-port-768--srv-overview.png` | Cartes pleine largeur sur 700 px pour 3 mots |
| `mobile-390--dashboard.png` | 425 px de cartes pour afficher « 1, 0, 0, 0 » |

Ces captures ne sont pas versionnées (elles sont régénérées à chaque exécution). Pour les reproduire :

```bash
cd frontend && bun run audit:ui
```

Le harnais est versionné dans `frontend/e2e/ui-audit.spec.ts` pour la variante complète avec captures, et dans `frontend/e2e/responsive-guardrails.spec.ts` pour la variante bloquante en CI (lot 6, point 32).

---

## 15. Conclusion

Le diagnostic de l'auteur est juste, mais la cause n'est pas celle qu'on pourrait supposer. Il ne s'agit pas d'un manque de soin : le lint est propre, le typage est strict, le backend est rigoureux, la sécurité est sérieusement traitée. Il s'agit d'une **application conçue, développée et validée sur un seul format d'écran**, dont l'unique boucle de retour visuel — `design-qa.md` et ses captures 1680×1080 — ne pouvait par construction rien révéler des autres.

Les trois défauts qui pèsent le plus sur la perception quotidienne sont bon marché à corriger : **les champs à 16 px** (une règle CSS), **les grilles de statistiques en 2 colonnes sur mobile** (deux règles, ~250 px d'écran récupérés sur chaque page), et **les cibles tactiles à 44 px** (un bloc `@media`). À eux seuls, ils changent radicalement l'expérience mobile pour moins d'une journée de travail.

Les trois qui pèsent le plus sur la **fiabilité** sont l'éditeur Monaco inopérant en production, le journal d'audit amputé sur mobile, et la console non virtualisée.

Et le correctif le plus rentable à long terme est le point 32 : brancher le harnais de mesure de cet audit en test de non-régression. Sans lui, chaque lot corrigé se dégradera au rythme des fonctionnalités suivantes, exactement comme aujourd'hui.
