import { useEffect, useRef, useState } from "react";
import { ToastMessage, useToast } from "@/contexts/ToastContext";
import { useLanguage } from "@/contexts/LanguageContext";
import { X, CheckCircle, AlertTriangle, AlertCircle, Info } from "lucide-react";

export default function Toast({ toast }: { toast: ToastMessage }) {
    const { removeToast } = useToast();
    const { t } = useLanguage();
    // WCAG 2.2.1 : un message qui disparaît seul doit pouvoir être retenu. Le
    // décompte se suspend au survol et tant que le focus est dans la notification.
    const [held, setHeld] = useState(false);
    const timerRef = useRef<number | null>(null);

    useEffect(() => {
        if (toast.duration <= 0 || held) return;
        timerRef.current = window.setTimeout(() => removeToast(toast.id), toast.duration);
        return () => {
            if (timerRef.current !== null) window.clearTimeout(timerRef.current);
        };
    }, [held, removeToast, toast.duration, toast.id]);

    return (
        <div
            className={`toast toast--${toast.type}`}
            onMouseEnter={() => setHeld(true)}
            onMouseLeave={() => setHeld(false)}
            onFocusCapture={() => setHeld(true)}
            onBlurCapture={() => setHeld(false)}
        >
            <div className="toast__icon" aria-hidden="true">
                {toast.type === "success" && <CheckCircle size={20} />}
                {toast.type === "error" && <AlertCircle size={20} />}
                {toast.type === "warning" && <AlertTriangle size={20} />}
                {toast.type === "info" && <Info size={20} />}
            </div>
            <div className="toast__content">
                {toast.message}
            </div>
            <button
                type="button"
                className="toast__close"
                aria-label={t("common.close")}
                onClick={() => removeToast(toast.id)}
            >
                <X size={16} aria-hidden="true" />
            </button>
        </div>
    );
}
