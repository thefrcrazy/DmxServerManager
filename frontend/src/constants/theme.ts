export const PRESET_COLORS = [
    "#3A82F6", // Default Blue
    "#FF591E", // Mistral Orange
    "#6366F1", // Indigo
    "#ec4899", // Pink
    "#10B981", // Emerald
    "#F59E0B", // Amber
];

export const LANGUAGES = [
    { code: "fr", name: "Français" },
    { code: "en", name: "English" },
];

export const applyAccentColor = (color: string): void => {
    const root = document.documentElement;
    root.style.setProperty("--color-accent", color);
    const r = parseInt(color.slice(1, 3), 16);
    const g = parseInt(color.slice(3, 5), 16);
    const b = parseInt(color.slice(5, 7), 16);
    root.style.setProperty("--color-accent-rgb", `${r}, ${g}, ${b}`);
};
