import { FormEvent, useCallback, useEffect, useState } from "react";
import { KeyRound, PackageSearch, ShieldCheck, Trash2 } from "lucide-react";
import { Button } from "@/components/ui";
import { useDialog } from "@/contexts/DialogContext";
import { useLanguage } from "@/contexts/LanguageContext";
import { useToast } from "@/contexts/ToastContext";
import { ModProviderStatus } from "@/schemas/operations";
import { apiService } from "@/services";

export default function ModProviderManagement() {
    const { t } = useLanguage();
    const toast = useToast();
    const { confirm } = useDialog();
    const [status, setStatus] = useState<ModProviderStatus | null>(null);
    const [apiKey, setApiKey] = useState("");
    const [loading, setLoading] = useState(true);
    const [saving, setSaving] = useState(false);
    const [error, setError] = useState("");

    const load = useCallback(async () => {
        const response = await apiService.mods.providerStatus();
        setLoading(false);
        if (!response.success) {
            setError(response.error.message);
            return;
        }
        setStatus(response.data);
        setError("");
    }, []);

    useEffect(() => { void load(); }, [load]);

    const save = async (event: FormEvent<HTMLFormElement>) => {
        event.preventDefault();
        if (apiKey.length < 16 || apiKey.length > 512 || apiKey.trim() !== apiKey) {
            setError(t("administration.mod_providers.invalid_key"));
            return;
        }
        setSaving(true);
        setError("");
        const writeOnlyKey = apiKey;
        setApiKey("");
        const response = await apiService.mods.configureCurseForge(writeOnlyKey);
        setSaving(false);
        if (!response.success) {
            setError(response.error.message);
            return;
        }
        setStatus((current) => current ? { ...current, curseforge: response.data } : current);
        toast.success(t("administration.mod_providers.saved"));
    };

    const clear = async () => {
        if (!await confirm(t("administration.mod_providers.delete_confirm"), { isDestructive: true })) return;
        setSaving(true);
        const response = await apiService.mods.clearCurseForge();
        setSaving(false);
        if (!response.success) {
            setError(response.error.message);
            return;
        }
        setStatus((current) => current ? { ...current, curseforge: response.data } : current);
        toast.success(t("administration.mod_providers.deleted"));
    };

    if (loading) {
        return <div className="administration-loading" role="status"><span className="spinner spinner--sm" />{t("common.loading")}</div>;
    }

    return (
        <section className="administration-panel mod-provider-management" aria-labelledby="mod-providers-heading">
            <div className="administration-panel__heading">
                <div><h2 id="mod-providers-heading">{t("administration.mod_providers.title")}</h2><p>{t("administration.mod_providers.description")}</p></div>
            </div>
            {error && <div className="administration-alert administration-alert--error" role="alert">{error}</div>}
            <div className="mod-provider-settings">
                <article className="card mod-provider-setting">
                    <header><PackageSearch size={22} aria-hidden="true" /><div><h3>Modrinth</h3><span className="badge badge--success">{t("administration.mod_providers.ready")}</span></div></header>
                    <p>{t("administration.mod_providers.modrinth_hint")}</p>
                    <div className="administration-notice"><ShieldCheck size={16} aria-hidden="true" />{t("administration.mod_providers.integrity")}</div>
                </article>
                <article className="card mod-provider-setting">
                    <header><KeyRound size={22} aria-hidden="true" /><div><h3>CurseForge</h3><span className={`badge badge--${status?.curseforge.configured ? "success" : "muted"}`}>{t(status?.curseforge.configured ? "administration.mod_providers.configured" : "administration.mod_providers.not_configured")}</span></div></header>
                    <p>{t("administration.mod_providers.curseforge_hint")}</p>
                    <form onSubmit={(event) => void save(event)}>
                        <div className="form-group">
                            <label htmlFor="curseforge-api-key">{t("administration.mod_providers.api_key")}</label>
                            <input id="curseforge-api-key" className="input" type="password" value={apiKey} minLength={16} maxLength={512} autoComplete="new-password" spellCheck={false} required onChange={(event) => setApiKey(event.target.value)} />
                            <small>{t("administration.mod_providers.write_only")}</small>
                        </div>
                        <div className="mod-provider-setting__actions">
                            {status?.curseforge.configured && <Button type="button" variant="danger" icon={<Trash2 size={16} aria-hidden="true" />} disabled={saving} onClick={() => void clear()}>{t("administration.mod_providers.delete_key")}</Button>}
                            <Button type="submit" isLoading={saving} disabled={!apiKey}>{t("common.save")}</Button>
                        </div>
                    </form>
                </article>
            </div>
        </section>
    );
}
