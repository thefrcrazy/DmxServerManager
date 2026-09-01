# QA UI/UX — 1er septembre 2026

## Résultat

Réussi. Les garde-fous mesurés passent sur les trois viewports explicites et sur
les trois appareils émulés, WebKit compris.

## Ce qui est réellement vérifié

Cette page décrit la couverture **mesurée**, pas une impression de relecture.
La version précédente concluait « aucun problème P0/P1/P2 » et déclarait le
responsive vérifié, alors que ses neuf captures de preuve étaient toutes en
1680×1080 ou 1542×1080. Aucune ne montrait un téléphone ni une tablette.

### Garde-fous bloquants

`frontend/e2e/responsive-guardrails.spec.ts` et
`frontend/e2e/device-guardrails.spec.ts` mesurent, sur **11 routes** :

| Invariant | Seuil | Motif |
|---|---|---|
| Largeur du document | ≤ viewport | aucun défilement horizontal de page |
| Bord droit de chaque élément visible | ≤ viewport | aucun contenu hors cadre |
| `font-size` des champs de saisie | ≥ 16 px sur tactile | Safari iOS zoome irréversiblement en dessous |
| Boîte de chaque élément interactif | ≥ 44 × 44 px sur tactile | WCAG 2.5.5 / Apple HIG |
| axe-core | 0 violation | `wcag2a`, `wcag2aa`, `wcag21a`, `wcag21aa`, `wcag22aa` |

Les zones tactiles étendues sont prises en compte : l'étiquette d'une case à
cocher et le lien étendu d'une carte sont mesurés à leur surface réellement
atteignable, pas à la boîte du contrôle.

### Moteurs et formats

| Projet Playwright | Moteur | Portée |
|---|---|---|
| `chromium` (Desktop Chrome) | Chromium | suite complète, plus les garde-fous en 390×844, 768×1024 et 1366×768 |
| `mobile-safari` (iPhone 13) | **WebKit** | garde-fous au viewport et au toucher de l'appareil |
| `mobile-chrome` (Pixel 5) | Chromium | idem |
| `tablet` (iPad gen 7) | Chromium | idem |

WebKit est le moteur où se manifestent le zoom automatique iOS, le comportement
de `dvh` face à la barre d'URL et les zones de sécurité : les vérifier sous
Chromium seul ne prouvait rien.

### Autres tests liés à l'interface

- `no-external-requests.spec.ts` — échoue sur toute requête sortante hors
  origine, en dehors des deux CDN d'illustrations explicitement autorisés.
  C'est ce test qui aurait attrapé le chargement de Monaco depuis jsDelivr.
- `tests/components.test.tsx` — montage réel des primitives corrigées : état
  désactivé du bouton lien, liaison du message d'erreur au champ, motif ARIA des
  onglets, régions live des notifications.
- `tests/i18n.test.ts` — parité stricte des clés FR/EN, et vérification que
  toute clé littérale employée dans le code existe. `t()` retombant sur le
  chemin brut, une clé manquante s'affichait telle quelle sans rien casser.
- `tests/mergeLogHistory.test.ts` — fusion d'historique de journal, dont un cas
  de volume qui verrouille la complexité linéaire.

## Reproduire

```bash
cd frontend && bunx playwright install --with-deps chromium webkit && bun run test:e2e
```

Captures complètes sur sept viewports, non versionnées :

```bash
cd frontend && bun run audit:ui
```

## Limites connues

- Les garde-fous couvrent 11 routes sur les écrans principaux ; les modales et
  les formulaires d'administration secondaires ne sont pas parcourus
  automatiquement.
- axe-core ne détecte pas l'absence de piège de focus, la navigation aux
  flèches d'un motif ARIA, ni un contenu masqué par `display: none` en
  responsive. Ces points relèvent des tests de composants et de la relecture.
- `services::runtime::tests::dropping_managed_process_kills_its_process_group`
  est intermittent sous charge parallèle (temporisation de terminaison de groupe
  de processus). Il n'est pas lié à l'interface.

## Référence

L'audit détaillé qui a motivé ces correctifs : `AUDIT-UI-UX-2026-08-31.md`.
