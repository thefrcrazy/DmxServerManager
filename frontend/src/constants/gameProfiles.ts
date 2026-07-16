export interface GameProfileVisual {
    artwork: string;
    label: string;
}

const visuals: Record<string, GameProfileVisual> = {
    hytale: { artwork: "/game-art/hytale.svg", label: "Hytale" },
    "minecraft-bedrock": { artwork: "/game-art/minecraft-bedrock.svg", label: "Minecraft Bedrock" },
    valheim: { artwork: "/game-art/valheim.svg", label: "Valheim" },
    palworld: { artwork: "/game-art/palworld.svg", label: "Palworld" },
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
