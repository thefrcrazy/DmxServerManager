import React, { useState, useEffect, useRef } from "react";
import { useDialog } from "@/contexts/DialogContext";
import { AlertTriangle, Info, HelpCircle } from "lucide-react";

export default function DialogContainer() {
    const { activeDialog, closeDialog } = useDialog();
    const [inputValue, setInputValue] = useState("");
    const inputRef = useRef<HTMLInputElement>(null);

    useEffect(() => {
        if (activeDialog) {
            setInputValue(activeDialog.defaultValue || "");
            if (activeDialog.type === "prompt") {
                setTimeout(() => inputRef.current?.focus(), 100);
            }
        }
    }, [activeDialog]);

    if (!activeDialog) return null;

    const handleConfirm = () => {
        if (activeDialog.verificationString && inputValue !== activeDialog.verificationString) return;
        
        if (activeDialog.type === "prompt") {
            closeDialog(inputValue);
        } else {
            closeDialog(true);
        }
    };

    const handleCancel = () => {
        if (activeDialog.type === "prompt") {
            closeDialog(null);
        } else {
            closeDialog(false);
        }
    };

    const isConfirmDisabled = activeDialog.verificationString ? inputValue !== activeDialog.verificationString : false;

    const handleKeyDown = (e: React.KeyboardEvent) => {
        if (e.key === "Enter" && !isConfirmDisabled) handleConfirm();
        if (e.key === "Escape" && activeDialog.type !== "alert") handleCancel();
    };

    return (
        <div className="dialog-overlay" onClick={activeDialog.type !== "alert" ? handleCancel : undefined}>
            <div className="dialog" onClick={(e) => e.stopPropagation()}>
                <div className="dialog__header">
                    <div className={`dialog__icon dialog__icon--${activeDialog.type}`}>
                        {activeDialog.isDestructive ? (
                            <AlertTriangle size={24} />
                        ) : (
                            <>
                                {activeDialog.type === "alert" && <Info size={24} />}
                                {activeDialog.type === "confirm" && <HelpCircle size={24} />}
                                {activeDialog.type === "prompt" && <Info size={24} />}
                            </>
                        )}
                    </div>
                    <h3 className="dialog__title">{activeDialog.title}</h3>
                </div>

                <div className="dialog__body">
                    <p>{activeDialog.message}</p>
                    {activeDialog.type === "prompt" && (
                        <input
                            ref={inputRef}
                            type="text"
                            className="input"
                            value={inputValue}
                            onChange={(e) => setInputValue(e.target.value)}
                            onKeyDown={handleKeyDown}
                        />
                    )}
                    {activeDialog.verificationString && (
                        <div className="verification-field mt-4">
                            <label className="field-label mb-2">
                                {activeDialog.verificationLabel || "Veuillez saisir le texte de confirmation"}
                            </label>
                            <input
                                ref={inputRef}
                                type="text"
                                className="input"
                                placeholder={activeDialog.verificationString}
                                value={inputValue}
                                onChange={(e) => setInputValue(e.target.value)}
                                onKeyDown={handleKeyDown}
                                autoFocus
                            />
                        </div>
                    )}
                </div>

                <div className="dialog__footer">
                    {activeDialog.type !== "alert" && (
                        <button className="btn btn--ghost" onClick={handleCancel}>
                            {activeDialog.cancelLabel}
                        </button>
                    )}
                    <button
                        className={`btn ${activeDialog.isDestructive ? "btn--danger" : "btn--primary"}`}
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
