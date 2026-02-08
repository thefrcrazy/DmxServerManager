import React, { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";

interface ModalProps {
    isOpen: boolean;
    onClose: () => void;
    title: React.ReactNode;
    children: React.ReactNode;
    footer?: React.ReactNode;
    size?: "sm" | "md" | "lg" | "full";
    isDanger?: boolean;
    className?: string;
}

const Modal: React.FC<ModalProps> = ({
    isOpen,
    onClose,
    title,
    children,
    footer,
    size = "md",
    isDanger = false,
    className = ""
}) => {
    const [mounted, setMounted] = useState(false);
    const modalRef = useRef<HTMLDivElement>(null);

    useEffect(() => {
        setMounted(true);
        if (isOpen) {
            document.body.style.overflow = "hidden";
        }
        return () => {
            document.body.style.overflow = "unset";
        };
    }, [isOpen]);

    // Close on escape
    useEffect(() => {
        const handleEscape = (e: KeyboardEvent) => {
            if (e.key === "Escape" && isOpen) {
                onClose();
            }
        };
        document.addEventListener("keydown", handleEscape);
        return () => document.removeEventListener("keydown", handleEscape);
    }, [isOpen, onClose]);

    if (!mounted || !isOpen) return null;

    const sizeClass = size === "md" ? "" : `modal--${size}`;
    const dangerClass = isDanger ? "modal--danger" : "";

    return createPortal(
        <div className="modal-overlay" onClick={onClose}>
            <div 
                className={`modal ${sizeClass} ${dangerClass} ${className}`} 
                ref={modalRef}
                onClick={e => e.stopPropagation()}
                role="dialog"
                aria-modal="true"
            >
                <div className="modal__header">
                    <div className="modal__title">{title}</div>
                    <button className="modal__close" onClick={onClose} aria-label="Close">
                        <X size={20} />
                    </button>
                </div>
                <div className="modal__body">
                    {children}
                </div>
                {footer && (
                    <div className="modal__footer">
                        {footer}
                    </div>
                )}
            </div>
        </div>,
        document.body
    );
};

export default Modal;
