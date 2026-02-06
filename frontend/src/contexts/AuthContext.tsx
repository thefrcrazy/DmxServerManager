import { createContext, useContext, useState, useEffect, ReactNode } from "react";
import { apiService } from "@/services";

export interface User {
    id: string;
    username: string;
    role: string;
    accent_color?: string;
    must_change_password?: boolean;
}

interface AuthContextType {
    user: User | null;
    token: string | null;
    isLoading: boolean;
    login: (username: string, password: string) => Promise<void>;
    loginWithDiscord: () => void;
    logout: () => void;
    updateUser: (updates: Partial<User>) => void;
}

const AuthContext = createContext<AuthContextType | undefined>(undefined);

// Helper to apply user's accent color
const applyUserAccentColor = (color: string | undefined | null) => {
    if (color) {
        const root = document.documentElement;
        root.style.setProperty("--color-accent", color);
        // Convert hex to rgb for opacity support
        const r = parseInt(color.slice(1, 3), 16);
        const g = parseInt(color.slice(3, 5), 16);
        const b = parseInt(color.slice(5, 7), 16);
        root.style.setProperty("--color-accent-rgb", `${r}, ${g}, ${b}`);
        localStorage.setItem("draveur_accent_color", color);
    }
};

export function AuthProvider({ children }: { children: ReactNode }) {
    const [user, setUser] = useState<User | null>(null);
    const [token, setToken] = useState<string | null>(null);
    const [isLoading, setIsLoading] = useState(true);

    useEffect(() => {
        // Check for existing token
        const savedToken = localStorage.getItem("token");
        const savedUser = localStorage.getItem("user");

        if (savedToken && savedUser) {
            setToken(savedToken);
            try {
                const parsedUser = JSON.parse(savedUser);
                setUser(parsedUser);
            } catch (e) {
                console.error("Failed to parse user", e);
            }
        }
        setIsLoading(false);
    }, []);

    const login = async (username: string, password: string) => {
        const response = await apiService.auth.login(username, password);

        if (!response.success) {
            throw new Error(response.error?.error || "Login failed");
        }

        const data = response.data;
        setToken(data.token);
        setUser(data.user);
        localStorage.setItem("token", data.token);
        localStorage.setItem("user", JSON.stringify(data.user));

        // Apply user's accent color immediately after login
        applyUserAccentColor(data.user.accent_color);
    };

    const loginWithDiscord = () => {
        // Redirect to Discord OAuth2
        const clientId = import.meta.env.VITE_DISCORD_CLIENT_ID;
        const redirectUri = encodeURIComponent(`${window.location.origin}/auth/discord/callback`);
        const scope = encodeURIComponent("identify email");

        window.location.href = `https://discord.com/api/oauth2/authorize?client_id=${clientId}&redirect_uri=${redirectUri}&response_type=code&scope=${scope}`;
    };

    const logout = () => {
        setToken(null);
        setUser(null);
        localStorage.removeItem("token");
        localStorage.removeItem("user");
    };

    const updateUser = (updates: Partial<User>) => {
        if (user) {
            const updatedUser = { ...user, ...updates };
            setUser(updatedUser);
            localStorage.setItem("user", JSON.stringify(updatedUser));
            // Apply accent color if it was updated
            if (updates.accent_color) {
                applyUserAccentColor(updates.accent_color);
            }
        }
    };

    return (
        <AuthContext.Provider value={{ user, token, isLoading, login, loginWithDiscord, logout, updateUser }}>
            {children}
        </AuthContext.Provider>
    );
}

export function useAuth() {
    const context = useContext(AuthContext);
    if (context === undefined) {
        throw new Error("useAuth must be used within an AuthProvider");
    }
    return context;
}