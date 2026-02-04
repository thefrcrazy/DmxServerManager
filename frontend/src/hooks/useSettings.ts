import { useState, useEffect, useCallback } from 'react';
import apiService from '../services/api';

interface Settings {
    version: string;
    servers_dir: string;
    backups_dir: string;
    webhook_url?: string;
}

interface UseSettingsReturn {
    settings: Settings | null;
    loading: boolean;
    error: string | null;
    saving: boolean;
    refresh: () => Promise<void>;
    updateWebhook: (url: string) => Promise<boolean>;
}

export function useSettings(): UseSettingsReturn {
    const [settings, setSettings] = useState<Settings | null>(null);
    const [loading, setLoading] = useState(true);
    const [saving, setSaving] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const refresh = useCallback(async () => {
        setLoading(true);
        setError(null);
        try {
            const response = await apiService.getSettings();
            if (response.data) {
                setSettings({
                    version: response.data.version || '',
                    servers_dir: response.data.servers_dir || '',
                    backups_dir: response.data.backups_dir || '',
                    webhook_url: response.data.webhook_url,
                });
            }
        } catch (err) {
            setError(err instanceof Error ? err.message : 'Erreur de chargement');
        } finally {
            setLoading(false);
        }
    }, []);

    useEffect(() => {
        refresh();
    }, [refresh]);

    const updateWebhook = useCallback(async (url: string): Promise<boolean> => {
        setSaving(true);
        try {
            await apiService.updateSettings({ webhook_url: url });
            setSettings(prev => prev ? { ...prev, webhook_url: url } : null);
            return true;
        } catch {
            return false;
        } finally {
            setSaving(false);
        }
    }, []);

    return {
        settings,
        loading,
        error,
        saving,
        refresh,
        updateWebhook,
    };
}
