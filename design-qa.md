# UI/UX QA — 2026-07-22

## Périmètre

- Dashboard, liste/grille des serveurs et détail d’une instance.
- Activité et panneau latéral de détail.
- Configuration d’instance, fichiers natifs et zone de danger.
- Desktop `1648 × 1076` et mobile `390 × 844`.

## Vérifications visuelles

- Rythme d’espacement harmonisé sur une base de 4 px, sans chevauchement ni débordement horizontal.
- Les quatre indicateurs du dashboard tiennent dans la largeur desktop.
- Les cartes et formulaires restent lisibles en mobile, avec actions empilées.
- Le panneau Activité entre depuis la droite avec fondu du voile, sort avant démontage et couvre tout l’écran en mobile.
- Les détails des fichiers natifs affichent les métadonnées, un aperçu structuré puis un formulaire minimal sur demande.
- Les secrets restent masqués et une valeur vide conserve la valeur configurée.
- Sauvegarder appartient au bloc de configuration ; Supprimer est isolé dans une zone de danger distincte.

## Vérifications fonctionnelles

- Modification et mise en file d’un brouillon JSON validées dans un environnement isolé.
- État `En attente` affiché après mise en file.
- Préservation des clés inconnues, commentaires et valeurs secrètes couverte par tests JSON, properties/INI, Rust, Palworld et XML.
- Navigation et rendu sans débordement horizontal à `1648 × 1076` et `390 × 844`.

## Validation automatisée

- `bun run typecheck`
- `bun run lint`
- `bun test tests` — 54 tests réussis
- `bun run build`
- `git diff --check`

final result: passed
