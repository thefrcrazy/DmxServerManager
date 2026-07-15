import { ThemeTokensSchema, type ThemeTokens } from "../schemas/catalog";

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

export const DEFAULT_THEME_TOKENS: ThemeTokens = {
    accent: "#3A82F6",
    bg_primary: "#000000",
    bg_secondary: "#0A0A0A",
    bg_tertiary: "#111111",
    bg_elevated: "#161616",
    border: "#27272A",
    border_hover: "#71717A",
    text_primary: "#FFFFFF",
    text_secondary: "#D4D4D8",
    text_muted: "#A1A1AA",
    success: "#10B981",
    warning: "#F59E0B",
    danger: "#EF4444",
    info: "#3B82F6",
};

const TOKEN_VARIABLES: ReadonlyArray<readonly [keyof ThemeTokens, string]> = [
    ["accent", "--color-accent"],
    ["bg_primary", "--color-bg-primary"],
    ["bg_secondary", "--color-bg-secondary"],
    ["bg_tertiary", "--color-bg-tertiary"],
    ["bg_elevated", "--color-bg-elevated"],
    ["border", "--color-border"],
    ["border_hover", "--color-border-hover"],
    ["text_primary", "--color-text-primary"],
    ["text_secondary", "--color-text-secondary"],
    ["text_muted", "--color-text-muted"],
    ["success", "--color-success"],
    ["warning", "--color-warning"],
    ["danger", "--color-danger"],
    ["info", "--color-info"],
];

function rgbChannels(color: string): string {
    return [1, 3, 5]
        .map((offset) => Number.parseInt(color.slice(offset, offset + 2), 16))
        .join(", ");
}

export const applyThemeTokens = (input: ThemeTokens, accentOverride?: string): boolean => {
    const parsed = ThemeTokensSchema.safeParse(input);
    if (!parsed.success) return false;
    const tokens = {
        ...parsed.data,
        ...(accentOverride && /^#[0-9a-f]{6}$/i.test(accentOverride)
            ? { accent: accentOverride }
            : {}),
    };
    const root = document.documentElement;
    for (const [token, variable] of TOKEN_VARIABLES) {
        root.style.setProperty(variable, tokens[token]);
    }
    for (const token of ["accent", "success", "warning", "danger", "info"] as const) {
        root.style.setProperty(`--color-${token}-rgb`, rgbChannels(tokens[token]));
    }
    applyAccentColor(tokens.accent);
    return true;
};

export const applyAccentColor = (color: string): void => {
    if (!/^#[0-9a-f]{6}$/i.test(color)) return;
    const root = document.documentElement;
    root.style.setProperty("--color-accent", color);
    const r = Number.parseInt(color.slice(1, 3), 16);
    const g = Number.parseInt(color.slice(3, 5), 16);
    const b = Number.parseInt(color.slice(5, 7), 16);
    root.style.setProperty("--color-accent-rgb", `${r}, ${g}, ${b}`);

    const weights = [0.2126, 0.7152, 0.0722];
    const luminance = [r, g, b]
        .map((channel) => channel / 255)
        .map((channel) => channel <= 0.03928 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4)
        .reduce((total, channel, index) => total + channel * (weights[index] ?? 0), 0);
    const contrastWithBlack = (luminance + 0.05) / 0.05;
    const contrastWithWhite = 1.05 / (luminance + 0.05);
    root.style.setProperty(
        "--color-text-inverse",
        contrastWithBlack >= contrastWithWhite ? "#000000" : "#ffffff",
    );
};
