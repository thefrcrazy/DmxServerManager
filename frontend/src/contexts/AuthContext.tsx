import { createContext, ReactNode, useCallback, useContext, useEffect, useState } from "react";
import { apiService } from "@/services";

export interface User {
    id: string;
    username: string;
    role: string;
    permissions: string[];
    language: "fr" | "en";
    accent_color?: string;
    must_change_password: boolean;
}

interface AuthContextType {
    user: User | null;
    isLoading: boolean;
    login: (username: string, password: string) => Promise<void>;
    logout: () => Promise<void>;
    changePassword: (currentPassword: string, newPassword: string) => Promise<void>;
    refreshSession: () => Promise<void>;
    updateUser: (updates: Partial<User>) => void;
}

const AuthContext = createContext<AuthContextType | undefined>(undefined);

export const PASSWORD_CHANGED_FLASH_KEY = "dmx_server_manager_password_changed";
export const PASSWORD_CHANGED_EVENT = "dmx-password-changed";

export function AuthProvider({ children }: { children: ReactNode }) {
    const [user, setUser] = useState<User | null>(null);
    const [isLoading, setIsLoading] = useState(true);

    const refreshSession = useCallback(async () => {
        const response = await apiService.auth.me();
        if (!response.success) {
            setUser(null);
            return;
        }
        setUser(response.data.user);
    }, []);

    useEffect(() => {
        void refreshSession().finally(() => setIsLoading(false));
    }, [refreshSession]);

    const login = async (username: string, password: string) => {
        const response = await apiService.auth.login(username, password);
        if (!response.success) throw response.error;
        setUser(response.data.user);
    };

    const logout = useCallback(async () => {
        await apiService.auth.logout();
        setUser(null);
    }, []);

    const changePassword = useCallback(async (currentPassword: string, newPassword: string) => {
        const response = await apiService.auth.changePassword(currentPassword, newPassword);
        if (!response.success) throw response.error;
        sessionStorage.setItem(PASSWORD_CHANGED_FLASH_KEY, "1");
        window.dispatchEvent(new Event(PASSWORD_CHANGED_EVENT));
        setUser(null);
    }, []);

    useEffect(() => {
        const handleAuthRequired = () => setUser(null);
        window.addEventListener("dmx-auth-required", handleAuthRequired);
        return () => window.removeEventListener("dmx-auth-required", handleAuthRequired);
    }, []);

    useEffect(() => {
        const handlePasswordChangeRequired = () => {
            void refreshSession();
        };
        window.addEventListener("dmx-password-change-required", handlePasswordChangeRequired);
        return () => window.removeEventListener("dmx-password-change-required", handlePasswordChangeRequired);
    }, [refreshSession]);

    const updateUser = (updates: Partial<User>) => {
        setUser((current) => current ? { ...current, ...updates } : current);
    };

    return (
        <AuthContext.Provider value={{ user, isLoading, login, logout, changePassword, refreshSession, updateUser }}>
            {children}
        </AuthContext.Provider>
    );
}

export function useAuth() {
    const context = useContext(AuthContext);
    if (!context) throw new Error("useAuth must be used within an AuthProvider");
    return context;
}
