import { useAuth } from "@/contexts/AuthContext";

export function usePermission() {
    const { user } = useAuth();

    const hasPermission = (permission: string): boolean => {
        if (!user) return false;
        
        // Admin wildcard
        if (user.permissions.includes("*")) return true;

        // Direct match
        if (user.permissions.includes(permission)) return true;

        // Wildcard match (e.g. "server.*" matches "server.start")
        const parts = permission.split(".");
        for (let i = 1; i <= parts.length; i++) {
            const wildcard = parts.slice(0, i).join(".") + ".*";
            if (user.permissions.includes(wildcard)) return true;
        }

        return false;
    };

    return { hasPermission };
}
