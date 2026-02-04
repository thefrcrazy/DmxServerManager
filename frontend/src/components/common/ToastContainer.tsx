import React from 'react';
import { useToast } from '../../contexts/ToastContext';
import Toast from './Toast';

export default function ToastContainer() {
    const { toasts } = useToast();

    return (
        <div className="toast-container">
            {toasts.map((toast) => (
                <Toast key={toast.id} toast={toast} />
            ))}
        </div>
    );
}
