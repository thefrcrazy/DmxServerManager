import { useCallback } from "react";
import { useAuth } from "@/contexts/AuthContext";

export function usePermission() {
    const { user } = useAuth();

    const hasPermission = useCallback((permission: string): boolean => {
        if (!user) return false;
        return user.permissions.includes("*") || user.permissions.includes(permission);
    }, [user]);

    return { hasPermission };
}
