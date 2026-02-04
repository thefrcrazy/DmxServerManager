import React, { useEffect } from 'react';
import { ToastMessage, useToast } from '../../contexts/ToastContext';
import { X, CheckCircle, AlertTriangle, AlertCircle, Info } from 'lucide-react';

export default function Toast({ toast }: { toast: ToastMessage }) {
    const { removeToast } = useToast();

    // Auto-remove handled in context, but adding manual close
    return (
        <div className={`toast toast--${toast.type}`}>
            <div className="toast__icon">
                {toast.type === 'success' && <CheckCircle size={20} />}
                {toast.type === 'error' && <AlertCircle size={20} />}
                {toast.type === 'warning' && <AlertTriangle size={20} />}
                {toast.type === 'info' && <Info size={20} />}
            </div>
            <div className="toast__content">
                {toast.message}
            </div>
            <button className="toast__close" onClick={() => removeToast(toast.id)}>
                <X size={16} />
            </button>
        </div>
    );
}
