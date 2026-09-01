import { GlobalRegistrator } from "@happy-dom/global-registrator";

// Les tests de composants montent réellement l'arbre React : jusqu'ici aucun
// composant n'était rendu, seuls les clients API et les schémas étaient couverts.
GlobalRegistrator.register();
