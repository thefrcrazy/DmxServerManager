import { Globe2, Save } from "lucide-react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui";
import { useLanguage } from "@/contexts/LanguageContext";
import { useToast } from "@/contexts/ToastContext";
import type { NetworkSettings } from "@/schemas/operations";
import { apiService } from "@/services";

export default function NetworkManagement() {
    const { t } = useLanguage();
    const toast = useToast();
    const [settings, setSettings] = useState<NetworkSettings | null>(null);
    const [host, setHost] = useState("");
    const [loading, setLoading] = useState(true);
    const [saving, setSaving] = useState(false);
    const [error, setError] = useState("");

    useEffect(() => {
        void apiService.panel.network().then((response) => {
            if (!response.success) setError(response.error.message);
            else {
                setSettings(response.data);
                setHost(response.data.advertised_game_host ?? "");
            }
            setLoading(false);
        });
    }, []);

    const save = async (event: React.FormEvent) => {
        event.preventDefault();
        if (!settings) return;
        setSaving(true);
        setError("");
        const response = await apiService.panel.updateNetwork(host.trim() || null, settings.version);
        setSaving(false);
        if (!response.success) {
            setError(response.error.message);
            return;
        }
        setSettings(response.data);
        setHost(response.data.advertised_game_host ?? "");
        toast.success(t("administration.network.saved"));
    };

    if (loading) return <div className="administration-state"><span className="spinner" />{t("common.loading")}</div>;

    return (
        <section className="administration-section network-settings">
            <header className="administration-section__header">
                <div className="administration-section__title"><Globe2 size={20} /><div><h2>{t("administration.network.title")}</h2><p>{t("administration.network.subtitle")}</p></div></div>
            </header>
            <form className="card network-settings__form" onSubmit={save}>
                {error && <div className="alert alert--error" role="alert">{error}</div>}
                <div className="form-group">
                    <label className="form-label" htmlFor="advertised-game-host">{t("administration.network.host")}</label>
                    <input id="advertised-game-host" className="input" value={host} onChange={(event) => setHost(event.target.value)} placeholder="play.example.com" maxLength={253} autoComplete="off" spellCheck={false} />
                    <p className="form-hint">{t("administration.network.host_hint")}</p>
                </div>
                <div className="network-settings__preview"><span>{t("administration.network.preview")}</span><code>{host.trim() || t("administration.network.not_configured")}</code></div>
                <footer><Button type="submit" isLoading={saving} icon={<Save size={16} />}>{t("common.save")}</Button></footer>
            </form>
        </section>
    );
}
