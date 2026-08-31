import React, { useState, useEffect, useRef } from "react";
import { useDialog } from "@/contexts/DialogContext";
import { useLanguage } from "@/contexts/LanguageContext";
import { useFocusTrap } from "@/hooks/useFocusTrap";
import { AlertTriangle, Info, HelpCircle } from "lucide-react";

export default function DialogContainer() {
    const { activeDialog, closeDialog } = useDialog();
    const { t } = useLanguage();
    const [inputValue, setInputValue] = useState("");
    // Deux champs distincts : un dialogue `prompt` peut aussi porter une chaîne
    // de vérification, et une référence unique aurait été écrasée par le second.
    const promptRef = useRef<HTMLInputElement>(null);
    const verificationRef = useRef<HTMLInputElement>(null);

    const isDestructive = Boolean(activeDialog?.isDestructive);
    const verificationString = activeDialog?.verificationString;
    const isConfirmDisabled = verificationString ? inputValue !== verificationString : false;

    const handleCancel = () => {
        if (!activeDialog) return;
        closeDialog(activeDialog.type === "prompt" ? null : false);
    };

    const { containerRef, onKeyDown } = useFocusTrap<HTMLDivElement>({
        active: Boolean(activeDialog),
        onEscape: activeDialog && activeDialog.type !== "alert" ? handleCancel : undefined,
    });

    useEffect(() => {
        if (!activeDialog) return;
        setInputValue(activeDialog.defaultValue || "");
        // Le champ saisissable prend le focus s'il existe ; sinon le piège place
        // le focus sur le conteneur lui-même.
        const frame = requestAnimationFrame(() => {
            (verificationRef.current ?? promptRef.current)?.focus();
        });
        return () => cancelAnimationFrame(frame);
    }, [activeDialog]);

    if (!activeDialog) return null;

    const handleConfirm = () => {
        if (isConfirmDisabled) return;
        closeDialog(activeDialog.type === "prompt" ? inputValue : true);
    };

    const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
        if (event.key === "Enter" && !isConfirmDisabled) {
            handleConfirm();
            return;
        }
        onKeyDown(event);
    };

    return (
        <div
            className="dialog-overlay"
            onMouseDown={(event) => {
                if (event.target !== event.currentTarget) return;
                if (activeDialog.type !== "alert") handleCancel();
            }}
        >
            <div
                ref={containerRef}
                className="dialog"
                role="dialog"
                aria-modal="true"
                aria-labelledby={`dialog-title-${activeDialog.id}`}
                aria-describedby={`dialog-message-${activeDialog.id}`}
                tabIndex={-1}
                onKeyDown={handleKeyDown}
            >
                <div className="dialog__header">
                    <div className={`dialog__icon dialog__icon--${activeDialog.type}`}>
                        {isDestructive ? (
                            <AlertTriangle size={24} aria-hidden="true" />
                        ) : (
                            <>
                                {activeDialog.type === "alert" && <Info size={24} aria-hidden="true" />}
                                {activeDialog.type === "confirm" && <HelpCircle size={24} aria-hidden="true" />}
                                {activeDialog.type === "prompt" && <Info size={24} aria-hidden="true" />}
                            </>
                        )}
                    </div>
                    <h3 id={`dialog-title-${activeDialog.id}`} className="dialog__title">{activeDialog.title}</h3>
                </div>

                <div className="dialog__body">
                    <p id={`dialog-message-${activeDialog.id}`}>{activeDialog.message}</p>
                    {activeDialog.type === "prompt" && (
                        <input
                            ref={promptRef}
                            type="text"
                            className="input"
                            aria-label={activeDialog.title}
                            value={inputValue}
                            onChange={(event) => setInputValue(event.target.value)}
                        />
                    )}
                    {verificationString && (
                        <div className="verification-field mt-4">
                            <label htmlFor={`dialog-verification-${activeDialog.id}`} className="field-label mb-2">
                                {activeDialog.verificationLabel || t("common.confirmation_prompt")}
                            </label>
                            <input
                                ref={verificationRef}
                                id={`dialog-verification-${activeDialog.id}`}
                                type="text"
                                className="input"
                                placeholder={verificationString}
                                value={inputValue}
                                onChange={(event) => setInputValue(event.target.value)}
                            />
                        </div>
                    )}
                </div>

                <div className="dialog__footer">
                    {activeDialog.type !== "alert" && (
                        <button type="button" className="btn btn--ghost" onClick={handleCancel}>
                            {activeDialog.cancelLabel}
                        </button>
                    )}
                    <button
                        type="button"
                        className={`btn ${isDestructive ? "btn--danger" : "btn--primary"}`}
                        onClick={handleConfirm}
                        disabled={isConfirmDisabled}
                    >
                        {activeDialog.confirmLabel}
                    </button>
                </div>
            </div>
        </div>
    );
}
