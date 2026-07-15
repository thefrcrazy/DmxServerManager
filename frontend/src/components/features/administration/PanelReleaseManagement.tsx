import { useCallback, useEffect, useState } from "react";
import { CircleAlert, Clipboard, Container, Download, RefreshCw, ShieldCheck } from "lucide-react";
import { Button } from "@/components/ui";
import { useLanguage } from "@/contexts/LanguageContext";
import { useToast } from "@/contexts/ToastContext";
import { PanelReleaseStatus } from "@/schemas/releases";
import { apiService } from "@/services";

const STATUS_REFRESH_MS = 60_000;

export default function PanelReleaseManagement() {
    const { language, t } = useLanguage();
    const toast = useToast();
    const [status, setStatus] = useState<PanelReleaseStatus | null>(null);
    const [loading, setLoading] = useState(true);
    const [checking, setChecking] = useState(false);
    const [error, setError] = useState(false);

    const load = useCallback(async (showLoading = false) => {
        if (showLoading) setLoading(true);
        const response = await apiService.releases.status();
        if (showLoading) setLoading(false);
        if (!response.success) {
            setError(true);
            return;
        }
        setStatus(response.data);
        setError(false);
    }, []);

    useEffect(() => {
        void load(true);
        const timer = window.setInterval(() => void load(false), STATUS_REFRESH_MS);
        return () => window.clearInterval(timer);
    }, [load]);

    const checkNow = async () => {
        setChecking(true);
        setError(false);
        const response = await apiService.releases.check();
        setChecking(false);
        if (!response.success) {
            setError(true);
            return;
        }
        setStatus(response.data);
    };

    const copy = async (value: string) => {
        try {
            await navigator.clipboard.writeText(value);
            toast.success(t("administration.releases.copied"));
        } catch {
            toast.error(t("administration.releases.copy_error"));
        }
    };

    const formatDate = (value: string | null | undefined) => value
        ? new Intl.DateTimeFormat(language, { dateStyle: "medium", timeStyle: "short" }).format(new Date(value))
        : t("common.never");

    if (loading) {
        return <div className="administration-loading" role="status"><span className="spinner spinner--sm" />{t("common.loading")}</div>;
    }

    return (
        <section className="administration-panel panel-release-management" aria-labelledby="panel-release-heading">
            <div className="administration-panel__heading">
                <div><h2 id="panel-release-heading">{t("administration.releases.title")}</h2><p>{t("administration.releases.description")}</p></div>
                <Button type="button" onClick={() => void checkNow()} disabled={!status?.configured} isLoading={checking} icon={<RefreshCw size={18} />}>{t("administration.releases.check")}</Button>
            </div>

            {error && <div className="administration-alert administration-alert--error" role="alert">{t("administration.releases.load_error")}</div>}
            {status && !status.configured && <div className="card release-disabled" role="note"><CircleAlert size={22} aria-hidden="true" /><div><strong>{t("administration.releases.disabled")}</strong><p>{t("administration.releases.disabled_hint")}</p><code>DMX_RELEASE_MANIFEST_URL</code><code>DMX_RELEASE_PUBLIC_KEY</code></div></div>}

            {status?.configured && <div className="release-summary" aria-live="polite">
                <div className="card release-version"><span>{t("administration.releases.current")}</span><strong>v{status.current_version}</strong><small>{t(`administration.releases.deployment.${status.deployment_mode}`)}</small></div>
                <div className="card release-version"><span>{t("administration.releases.latest")}</span><strong>{status.latest ? `v${status.latest.version}` : "—"}</strong><small>{t(`administration.releases.states.${status.state}`)}</small></div>
                <div className="card release-version"><span>{t("administration.releases.checked_at")}</span><strong className="release-version__date">{formatDate(status.checked_at)}</strong><small>{status.error_code ? t(`administration.releases.errors.${status.error_code}`) : status.latest && status.state !== "checking" ? t("administration.releases.signature_verified") : t(`administration.releases.states.${status.state}`)}</small></div>
            </div>}

            {status?.state === "update_available" && status.latest && <article className="card release-details">
                <header><div className="release-details__icon">{status.latest.target.kind === "docker" ? <Container size={20} /> : <Download size={20} />}</div><div><h3>{t("administration.releases.instructions")}</h3><p>{t("administration.releases.published_at")} {formatDate(status.latest.published_at)}</p></div><span className="badge badge--success">{t("administration.releases.available")}</span></header>
                <div className="release-security"><ShieldCheck size={18} aria-hidden="true" /><p>{t("administration.releases.security_notice")}</p></div>

                {status.latest.target.kind === "native" ? <>
                    <dl className="release-metadata"><div><dt>{t("administration.releases.platform")}</dt><dd>{status.latest.target.platform}</dd></div><div><dt>{t("administration.releases.archive_checksum")}</dt><dd><code>{status.latest.target.archive_sha256}</code></dd></div><div><dt>{t("administration.releases.installer_checksum")}</dt><dd><code>{status.latest.target.installer_sha256}</code></dd></div></dl>
                    <a className="release-link" href={status.latest.target.installer_url} target="_blank" rel="noreferrer">{t("administration.releases.installer")}<Download size={15} /></a>
                    <Command value={status.latest.target.upgrade_command} label={t("administration.releases.native_command")} onCopy={copy} copyLabel={t("administration.releases.copy")} />
                </> : <>
                    <dl className="release-metadata"><div><dt>{t("administration.releases.image")}</dt><dd><code>{status.latest.target.image}</code></dd></div><div><dt>{t("administration.releases.digest")}</dt><dd><code>{status.latest.target.digest}</code></dd></div></dl>
                    <Command value={status.latest.target.pull_command} label={t("administration.releases.pull_command")} onCopy={copy} copyLabel={t("administration.releases.copy")} />
                    <Command value={status.latest.target.apply_command} label={t("administration.releases.apply_command")} onCopy={copy} copyLabel={t("administration.releases.copy")} />
                </>}
                <a className="release-link" href={status.latest.notes_url} target="_blank" rel="noreferrer">{t("administration.releases.notes")}</a>
            </article>}
        </section>
    );
}

function Command({ value, label, onCopy, copyLabel }: { value: string; label: string; onCopy: (value: string) => Promise<void>; copyLabel: string }) {
    return <div className="release-command"><div><strong>{label}</strong><Button type="button" size="sm" variant="ghost" icon={<Clipboard size={15} />} onClick={() => void onCopy(value)}>{copyLabel}</Button></div><pre tabIndex={0}><code>{value}</code></pre></div>;
}
