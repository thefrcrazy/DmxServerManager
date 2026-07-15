import { createContext, ReactNode, useCallback, useContext, useEffect, useMemo, useState } from "react";
import { applyThemeTokens, DEFAULT_THEME_TOKENS } from "@/constants/theme";
import { useAuth } from "@/contexts/AuthContext";
import type { ActiveTheme } from "@/schemas/catalog";
import { apiService } from "@/services";

type ThemeContextValue = {
    accentColor: string;
    setAccentColor: (color: string) => void;
    activeTheme: ActiveTheme;
    refreshTheme: () => Promise<void>;
};

const ThemeContext = createContext<ThemeContextValue | undefined>(undefined);
const DEFAULT_ACCENT = DEFAULT_THEME_TOKENS.accent;
const STORAGE_KEY = "dmx_server_manager_accent_color";
const DEFAULT_THEME: ActiveTheme = {
    selection: { kind: "default" },
    tokens: DEFAULT_THEME_TOKENS,
    assets: { logo: null, preview: null },
    version: 1,
    updated_at: "1970-01-01T00:00:00Z",
};

function validAccent(color: string | null | undefined): color is string {
    return typeof color === "string" && /^#[0-9a-f]{6}$/i.test(color);
}

export function ThemeProvider({ children }: { children: ReactNode }) {
    const { user } = useAuth();
    const [accentColor, setAccentColorState] = useState(() => {
        const stored = localStorage.getItem(STORAGE_KEY);
        return validAccent(stored) ? stored : DEFAULT_ACCENT;
    });
    const [activeTheme, setActiveTheme] = useState<ActiveTheme>(DEFAULT_THEME);

    const setAccentColor = useCallback((color: string) => {
        const safeColor = validAccent(color) ? color : DEFAULT_ACCENT;
        setAccentColorState(safeColor);
        localStorage.setItem(STORAGE_KEY, safeColor);
    }, []);

    const refreshTheme = useCallback(async () => {
        if (!user) {
            setActiveTheme(DEFAULT_THEME);
            return;
        }
        const response = await apiService.catalog.activeTheme();
        if (response.success) setActiveTheme(response.data);
    }, [user]);

    useEffect(() => {
        if (validAccent(user?.accent_color)) setAccentColor(user.accent_color);
    }, [setAccentColor, user?.accent_color]);

    useEffect(() => {
        void refreshTheme();
    }, [refreshTheme]);

    useEffect(() => {
        applyThemeTokens(activeTheme.tokens, accentColor);
    }, [accentColor, activeTheme.tokens]);

    const value = useMemo<ThemeContextValue>(() => ({
        accentColor,
        setAccentColor,
        activeTheme,
        refreshTheme,
    }), [accentColor, activeTheme, refreshTheme, setAccentColor]);

    return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme() {
    const context = useContext(ThemeContext);
    if (!context) throw new Error("useTheme must be used within a ThemeProvider");
    return context;
}
