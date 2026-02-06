import { createContext, useContext, useEffect, useState, ReactNode } from "react";

type Theme = {
    accentColor: string;
    setAccentColor: (color: string) => void;
};

const ThemeContext = createContext<Theme | undefined>(undefined);

// Default Blue (synchronized with backend)
const DEFAULT_ACCENT = "#3A82F6";

export function ThemeProvider({ children }: { children: ReactNode }) {
    const [accentColor, setAccentColor] = useState<string>(() => {
        // Priority: User's accent_color > localStorage theme > default
        const savedUser = localStorage.getItem("user");
        if (savedUser) {
            try {
                const user = JSON.parse(savedUser);
                if (user.accent_color) {
                    return user.accent_color;
                }
            } catch (e) {
                // Ignore parse errors
            }
        }
        return localStorage.getItem("draveur_accent_color") || DEFAULT_ACCENT;
    });

    useEffect(() => {
        const root = document.documentElement;

        // Set HEX color
        root.style.setProperty("--color-accent", accentColor);

        // Set RGB color (for opacities)
        // Convert hex to rgb
        const r = parseInt(accentColor.slice(1, 3), 16);
        const g = parseInt(accentColor.slice(3, 5), 16);
        const b = parseInt(accentColor.slice(5, 7), 16);
        root.style.setProperty("--color-accent-rgb", `${r}, ${g}, ${b}`);

        // Persist
        localStorage.setItem("draveur_accent_color", accentColor);
    }, [accentColor]);

    // Prevent flash of unstyled theme
    useEffect(() => {
        // Force immediate apply on mount
        const root = document.documentElement;
        root.style.setProperty("--color-accent", accentColor);
        const r = parseInt(accentColor.slice(1, 3), 16);
        const g = parseInt(accentColor.slice(3, 5), 16);
        const b = parseInt(accentColor.slice(5, 7), 16);
        root.style.setProperty("--color-accent-rgb", `${r}, ${g}, ${b}`);
    }, [accentColor]);

    // Sync with localStorage changes (from other components like AuthContext.updateUser)
    useEffect(() => {
        const syncFromStorage = () => {
            const savedUser = localStorage.getItem("user");
            if (savedUser) {
                try {
                    const user = JSON.parse(savedUser);
                    if (user.accent_color && user.accent_color.toLowerCase() !== accentColor.toLowerCase()) {
                        setAccentColor(user.accent_color);
                    }
                } catch (e) {
                    // Ignore parse errors
                }
            }
        };

        // Listen for storage events (works across tabs)
        window.addEventListener("storage", syncFromStorage);

        // Also check on route changes by polling (same tab updates don't trigger storage event)
        const interval = setInterval(syncFromStorage, 500);

        return () => {
            window.removeEventListener("storage", syncFromStorage);
            clearInterval(interval);
        };
    }, [accentColor]);

    return (
        <ThemeContext.Provider value={{ accentColor, setAccentColor }}>
            {children}
        </ThemeContext.Provider>
    );
}

export function useTheme() {
    const context = useContext(ThemeContext);
    if (context === undefined) {
        throw new Error("useTheme must be used within a ThemeProvider");
    }
    return context;
}
