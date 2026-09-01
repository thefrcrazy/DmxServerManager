import { createContext, useContext, useState, useCallback, ReactNode } from "react";

export type ToastType = "success" | "error" | "info" | "warning";

export interface ToastMessage {
    id: string;
    message: string;
    type: ToastType;
    /** 0 signifie « ne disparaît pas tout seul ». */
    duration: number;
}

interface ToastContextType {
    toasts: ToastMessage[];
    addToast: (message: string, type: ToastType, duration?: number) => void;
    removeToast: (id: string) => void;
    success: (message: string, duration?: number) => void;
    error: (message: string, duration?: number) => void;
    info: (message: string, duration?: number) => void;
    warning: (message: string, duration?: number) => void;
}

const ToastContext = createContext<ToastContextType | undefined>(undefined);

const DEFAULT_DURATION_MS = 5_000;

export function useToast() {
    const context = useContext(ToastContext);
    if (!context) {
        throw new Error("useToast must be used within a ToastProvider");
    }
    return context;
}

export function ToastProvider({ children }: { children: ReactNode }) {
    const [toasts, setToasts] = useState<ToastMessage[]>([]);

    const removeToast = useCallback((id: string) => {
        setToasts((prev) => prev.filter((toast) => toast.id !== id));
    }, []);

    const addToast = useCallback((message: string, type: ToastType, duration?: number) => {
        // Une erreur ne s'efface pas toute seule : c'est le canal principal de
        // retour d'échec, et cinq secondes ne suffisent ni à la lire ni à
        // atteindre sa fermeture au clavier.
        const resolved = duration ?? (type === "error" ? 0 : DEFAULT_DURATION_MS);
        setToasts((prev) => [...prev, { id: crypto.randomUUID(), message, type, duration: resolved }]);
    }, []);

    const success = useCallback((message: string, duration?: number) => addToast(message, "success", duration), [addToast]);
    const error = useCallback((message: string, duration?: number) => addToast(message, "error", duration), [addToast]);
    const info = useCallback((message: string, duration?: number) => addToast(message, "info", duration), [addToast]);
    const warning = useCallback((message: string, duration?: number) => addToast(message, "warning", duration), [addToast]);

    return (
        <ToastContext.Provider value={{ toasts, addToast, removeToast, success, error, info, warning }}>
            {children}
        </ToastContext.Provider>
    );
}
