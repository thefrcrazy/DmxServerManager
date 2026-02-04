import React, { createContext, useContext, useState, useCallback, ReactNode } from 'react';
import { v4 as uuidv4 } from 'uuid';

export type DialogType = 'alert' | 'confirm' | 'prompt';

export interface DialogOptions {
    title?: string;
    message: string;
    confirmLabel?: string;
    cancelLabel?: string;
    type?: DialogType;
    defaultValue?: string; // For prompt
    isDestructive?: boolean;
}

interface DialogRequest extends DialogOptions {
    id: string;
    resolve: (value: any) => void;
}

interface DialogContextType {
    alert: (message: string, options?: Omit<DialogOptions, 'message' | 'type'>) => Promise<void>;
    confirm: (message: string, options?: Omit<DialogOptions, 'message' | 'type'>) => Promise<boolean>;
    prompt: (message: string, options?: Omit<DialogOptions, 'message' | 'type'>) => Promise<string | null>;
    activeDialog: DialogRequest | null;
    closeDialog: (value: any) => void;
}

const DialogContext = createContext<DialogContextType | undefined>(undefined);

export function useDialog() {
    const context = useContext(DialogContext);
    if (!context) {
        throw new Error('useDialog must be used within a DialogProvider');
    }
    return context;
}

export function DialogProvider({ children }: { children: ReactNode }) {
    const [activeDialog, setActiveDialog] = useState<DialogRequest | null>(null);

    const openDialog = useCallback((type: DialogType, message: string, options: Partial<DialogOptions> = {}): Promise<any> => {
        return new Promise((resolve) => {
            setActiveDialog({
                id: uuidv4(),
                type,
                message,
                resolve,
                title: options.title || (type === 'confirm' ? 'Confirmation' : type === 'alert' ? 'Information' : 'Entrée'),
                confirmLabel: options.confirmLabel || 'OK',
                cancelLabel: options.cancelLabel || 'Annuler',
                defaultValue: options.defaultValue || '',
                isDestructive: options.isDestructive || false,
            });
        });
    }, []);

    const closeDialog = useCallback((value: any) => {
        if (activeDialog) {
            activeDialog.resolve(value);
            setActiveDialog(null);
        }
    }, [activeDialog]);

    const alert = useCallback((message: string, options = {}) => openDialog('alert', message, options), [openDialog]);
    const confirm = useCallback((message: string, options = {}) => openDialog('confirm', message, options), [openDialog]);
    const prompt = useCallback((message: string, options = {}) => openDialog('prompt', message, options), [openDialog]);

    return (
        <DialogContext.Provider value={{ alert, confirm, prompt, activeDialog, closeDialog }}>
            {children}
        </DialogContext.Provider>
    );
}
