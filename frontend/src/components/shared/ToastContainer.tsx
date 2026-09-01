import { useToast } from "@/contexts/ToastContext";
import Toast from "./Toast";

// Deux régions live distinctes, présentes en permanence dans le document : une
// région live insérée en même temps que son contenu n'est pas annoncée de façon
// fiable. Les erreurs sont assertives, le reste poli.
export default function ToastContainer() {
    const { toasts } = useToast();
    const errors = toasts.filter((toast) => toast.type === "error");
    const others = toasts.filter((toast) => toast.type !== "error");

    return (
        <div className="toast-container">
            <div role="alert" aria-live="assertive" aria-atomic="false" className="toast-region">
                {errors.map((toast) => <Toast key={toast.id} toast={toast} />)}
            </div>
            <div role="status" aria-live="polite" aria-atomic="false" className="toast-region">
                {others.map((toast) => <Toast key={toast.id} toast={toast} />)}
            </div>
        </div>
    );
}
