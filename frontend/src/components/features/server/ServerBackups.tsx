import { Archive, Download, LoaderCircle, RotateCcw, ShieldCheck, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Button } from "@/components/ui";
import { useDialog } from "@/contexts/DialogContext";
import { useLanguage } from "@/contexts/LanguageContext";
import { useToast } from "@/contexts/ToastContext";
import { Backup } from "@/schemas/operations";
import { apiService } from "@/services";
import { formatBytes } from "@/utils/formatters";

interface ServerBackupsProps {
    instanceId: string;
    canManage: boolean;
    isStopped: boolean;
    refreshSignal?: number;
}

export default function ServerBackups({ instanceId, canManage, isStopped, refreshSignal = 0 }: ServerBackupsProps) {
    const { t, language } = useLanguage();
    const toast = useToast();
    const { confirm } = useDialog();
    const [backups, setBackups] = useState<Backup[]>([]);
    const [loading, setLoading] = useState(true);
    const [busyId, setBusyId] = useState<string | null>(null);
    const [error, setError] = useState<string | null>(null);
    const canMutate = canManage && isStopped;

    const load = useCallback(async () => {
        const response = await apiService.backups.list(instanceId);
        setLoading(false);
        if (!response.success) {
            setError(response.error.message);
            return;
        }
        setBackups(response.data);
        setError(null);
    }, [instanceId]);

    useEffect(() => { void load(); }, [load, refreshSignal]);

    useEffect(() => {
        if (!backups.some((backup) => backup.status === "creating")) return;
        const timer = window.setTimeout(() => void load(), 2_000);
        return () => window.clearTimeout(timer);
    }, [backups, load]);

    const create = async () => {
        setBusyId("create");
        const response = await apiService.backups.create(instanceId, crypto.randomUUID());
        setBusyId(null);
        if (!response.success) return toast.error(response.error.message);
        toast.success(t("backups.creation_queued"));
        await load();
    };

    const restore = async (backup: Backup) => {
        const accepted = await confirm(t("backups.restore_confirm"), { isDestructive: true });
        if (!accepted) return;
        setBusyId(backup.id);
        const response = await apiService.backups.restore(backup.id, crypto.randomUUID());
        setBusyId(null);
        if (!response.success) return toast.error(response.error.message);
        toast.success(t("backups.restore_queued"));
    };

    const remove = async (backup: Backup) => {
        const accepted = await confirm(t("backups.delete_confirm"), { isDestructive: true });
        if (!accepted) return;
        setBusyId(backup.id);
        const response = await apiService.backups.remove(backup.id);
        setBusyId(null);
        if (!response.success) return toast.error(response.error.message);
        setBackups((current) => current.filter((item) => item.id !== backup.id));
        toast.success(t("backups.success_delete"));
    };

    return (
        <section className="server-backups card" aria-labelledby="backups-heading">
            <div className="server-backups__header">
                <div><h2 id="backups-heading">{t("backups.title")}</h2><p>{t("backups.instance_subtitle")}</p></div>
                {canManage && <Button icon={<Archive size={17} aria-hidden="true" />} disabled={!canMutate} isLoading={busyId === "create"} onClick={() => void create()}>{t("backups.create_backup")}</Button>}
            </div>
            {canManage && !isStopped && <p className="operations-warning" role="status">{t("backups.stop_to_manage")}</p>}
            {loading && <div className="operations-loading"><LoaderCircle className="spinner" aria-hidden="true" />{t("common.loading")}</div>}
            {error && <div className="operations-error" role="alert">{error}<Button size="sm" variant="secondary" onClick={() => void load()}>{t("administration.retry")}</Button></div>}
            {!loading && !error && backups.length === 0 && <div className="operations-empty"><Archive aria-hidden="true" /><p>{t("backups.empty_desc")}</p>{canMutate && <Button onClick={() => void create()}>{t("backups.create_first")}</Button>}</div>}
            {!loading && !error && backups.length > 0 && (
                <div className="table-scroll"><table className="backup-table">
                    <thead><tr><th>{t("backups.date")}</th><th>{t("backups.status")}</th><th>{t("backups.size")}</th><th>{t("backups.checksum")}</th><th>{t("common.actions")}</th></tr></thead>
                    <tbody>{backups.map((backup) => (
                        <tr key={backup.id}>
                            <td><time dateTime={backup.created_at}>{new Date(backup.created_at).toLocaleString(language === "fr" ? "fr-FR" : "en-US")}</time></td>
                            <td><span className={`badge badge--${backup.status === "ready" ? "success" : backup.status === "failed" ? "danger" : "warning"}`}>{t(`backups.statuses.${backup.status}`)}</span></td>
                            <td>{backup.size_bytes === null ? "—" : formatBytes(backup.size_bytes)}</td>
                            <td><code title={backup.checksum_sha256 ?? ""}>{backup.checksum_sha256 ? `${backup.checksum_sha256.slice(0, 12)}…` : "—"}</code></td>
                            <td><div className="backup-row-actions">
                                {backup.status === "ready" && <a className="icon-action" href={apiService.backups.downloadUrl(backup.id)} download={`dmx-backup-${backup.id}.zip`} aria-label={t("backups.download")}><Download size={15} aria-hidden="true" /></a>}
                                {canMutate && backup.status === "ready" && <button type="button" className="icon-action" disabled={busyId === backup.id} onClick={() => void restore(backup)} aria-label={t("backups.restore")}><RotateCcw size={15} aria-hidden="true" /></button>}
                                {canMutate && backup.status !== "creating" && <button type="button" className="icon-action icon-action--danger" disabled={busyId === backup.id} onClick={() => void remove(backup)} aria-label={t("common.delete")}><Trash2 size={15} aria-hidden="true" /></button>}
                            </div></td>
                        </tr>
                    ))}</tbody>
                </table></div>
            )}
            <p className="server-backups__integrity"><ShieldCheck size={16} aria-hidden="true" />{t("backups.integrity_hint")}</p>
        </section>
    );
}
