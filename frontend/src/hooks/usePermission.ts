import { useCallback } from "react";
import { useAuth } from "@/contexts/AuthContext";

export function usePermission() {
    const { user } = useAuth();

    const hasPermission = useCallback((permission: string): boolean => {
        if (!user) return false;
        
        // Admin role always has all permissions
        if (user.role === "admin") return true;

        if (!user.permissions) return false;
        
        // Ensure permissions is an array (handle potential string from storage)
        const perms = Array.isArray(user.permissions) 
            ? user.permissions 
            : typeof user.permissions === "string" 
                ? JSON.parse(user.permissions) 
                : [];

        // Admin wildcard
        if (perms.includes("*")) return true;

        // Direct match
        if (perms.includes(permission)) return true;

        // Wildcard match (e.g. "server.*" matches "server.start")
        const parts = permission.split(".");
        for (let i = 1; i <= parts.length; i++) {
            const wildcard = parts.slice(0, i).join(".") + ".*";
            if (perms.includes(wildcard)) return true;
        }

        return false;
    }, [user]);

    return { hasPermission };
}
