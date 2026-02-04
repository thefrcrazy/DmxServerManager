export const GAME_LOGOS: Record<string, string> = {
    hytale: "https://hytale.com/static/images/logo-h.png",
};

export const getGameLogo = (gameType: string): string | undefined => {
    return GAME_LOGOS[gameType.toLowerCase()];
};
