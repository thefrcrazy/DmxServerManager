import type { SyntheticEvent } from "react";

export interface GameProfileVisual {
    artwork: string;
    label: string;
}

const visuals: Record<string, GameProfileVisual> = {
    hytale: { artwork: "/game-art/hytale.svg", label: "Hytale" },
    "minecraft-java": { artwork: "/game-art/minecraft-java.svg", label: "Minecraft Java" },
    "minecraft-bedrock": { artwork: "/game-art/minecraft-bedrock.svg", label: "Minecraft Bedrock" },
    valheim: { artwork: "/game-art/valheim.svg", label: "Valheim" },
    palworld: { artwork: "/game-art/palworld.svg", label: "Palworld" },
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
            artwork: "/game-art/minecraft-java.svg",
            label: fallbackLabel ?? `Minecraft Java · ${loader.charAt(0).toUpperCase()}${loader.slice(1)}`,
        };
    }
    if (profileId.startsWith("steam-") || profileId === "steam_custom") {
        return { ...visuals.steam, label: fallbackLabel ?? visuals.steam.label };
    }
    const visual = visuals[profileId] ?? visuals.steam;
    return { ...visual, label: fallbackLabel ?? visual.label };
}

export function fallbackGameArtwork(event: SyntheticEvent<HTMLImageElement>): void {
    const image = event.currentTarget;
    image.onerror = null;
    image.src = visuals.steam.artwork;
}
