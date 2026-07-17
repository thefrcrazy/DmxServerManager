import type { SyntheticEvent } from "react";

export interface GameProfileVisual {
    artwork: string;
    fallbackArtwork?: string;
    artworkPosition?: string;
    label: string;
}

const visuals: Record<string, GameProfileVisual> = {
    hytale: {
        artwork: "https://static-cdn.jtvnw.net/ttv-boxart/1003606006_IGDB-600x800.jpg",
        fallbackArtwork: "/game-art/hytale.svg",
        artworkPosition: "center top",
        label: "Hytale",
    },
    "minecraft-java": {
        artwork: "https://static-cdn.jtvnw.net/ttv-boxart/27471_IGDB-600x800.jpg",
        fallbackArtwork: "/game-art/minecraft-java.svg",
        artworkPosition: "center top",
        label: "Minecraft Java",
    },
    "minecraft-bedrock": {
        artwork: "https://static-cdn.jtvnw.net/ttv-boxart/27471_IGDB-600x800.jpg",
        fallbackArtwork: "/game-art/minecraft-bedrock.svg",
        artworkPosition: "center top",
        label: "Minecraft Bedrock",
    },
    valheim: {
        artwork: "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/892970/header.jpg",
        fallbackArtwork: "/game-art/valheim.svg",
        label: "Valheim",
    },
    palworld: {
        artwork: "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/1623730/capsule_616x353.jpg",
        fallbackArtwork: "/game-art/palworld.svg",
        label: "Palworld",
    },
    satisfactory: {
        artwork: "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/526870/header.jpg",
        label: "Satisfactory",
    },
    "seven-days-to-die": {
        artwork: "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/251570/header.jpg",
        label: "7 Days to Die",
    },
    "project-zomboid": {
        artwork: "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/108600/header.jpg",
        label: "Project Zomboid",
    },
    rust: {
        artwork: "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/252490/header.jpg",
        label: "Rust",
    },
    steam: { artwork: "/game-art/steam.svg", label: "SteamCMD" },
};

export function gameProfileVisual(profileId: string, fallbackLabel?: string): GameProfileVisual {
    if (profileId.startsWith("minecraft-java-")) {
        const loader = profileId.slice("minecraft-java-".length);
        return {
            ...visuals["minecraft-java"],
            label: fallbackLabel ?? `Minecraft Java · ${loader.charAt(0).toUpperCase()}${loader.slice(1)}`,
        };
    }
    if (profileId.startsWith("steam-") || profileId === "steam_custom") {
        return { ...visuals.steam, label: fallbackLabel ?? visuals.steam.label };
    }
    const visual = visuals[profileId] ?? visuals.steam;
    return { ...visual, label: fallbackLabel ?? visual.label };
}

export function fallbackGameArtwork(event: SyntheticEvent<HTMLImageElement>, fallbackArtwork = visuals.steam.artwork): void {
    const image = event.currentTarget;
    image.onerror = null;
    image.src = fallbackArtwork;
}
