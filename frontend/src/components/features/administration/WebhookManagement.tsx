import { FormEvent, useCallback, useEffect, useState } from "react";
import { CircleAlert, Clock3, Pencil, Plus, ShieldCheck, Trash2, Webhook } from "lucide-react";
import { Button } from "@/components/ui";
import { useLanguage } from "@/contexts/LanguageContext";
import { useToast } from "@/contexts/ToastContext";
import {
    CreateDiscordWebhookSchema,
    DISCORD_WEBHOOK_EVENTS,
    DiscordWebhook,
    DiscordWebhookEvent,
    UpdateDiscordWebhookSchema,
} from "@/schemas/operations";
import { apiService } from "@/services";
import { ApiClientError } from "@/services/api/base.client";

function webhookError(error: ApiClientError, fallback: string, conflict: string): string {
    const message = error.status === 409 ? conflict : error.code ? `${fallback} (${error.code})` : error.message || fallback;
    return error.traceId ? `${message} · trace ${error.traceId}` : message;
}

export default function WebhookManagement() {
    const { language, t } = useLanguage();
    const toast = useToast();
    const [webhooks, setWebhooks] = useState<DiscordWebhook[]>([]);
    const [editing, setEditing] = useState<DiscordWebhook | null>(null);
    const [creating, setCreating] = useState(false);
    const [name, setName] = useState("");
    const [url, setUrl] = useState("");
    const [events, setEvents] = useState<DiscordWebhookEvent[]>(["job.failed"]);
    const [enabled, setEnabled] = useState(true);
    const [loading, setLoading] = useState(true);
    const [saving, setSaving] = useState(false);
    const [error, setError] = useState("");

    const load = useCallback(async () => {
        setError("");
        const response = await apiService.webhooks.list();
        setLoading(false);
        if (!response.success) {
            setError(webhookError(response.error, t("administration.webhooks.load_error"), t("administration.webhooks.conflict")));
            return;
        }
        setWebhooks(response.data);
    }, [t]);

    useEffect(() => { void load(); }, [load]);

    const reset = () => {
        setCreating(false);
        setEditing(null);
        setName("");
        setUrl("");
        setEvents(["job.failed"]);
        setEnabled(true);
        setError("");
    };

    const beginCreate = () => {
        reset();
        setCreating(true);
    };

    const beginEdit = (webhook: DiscordWebhook) => {
        setCreating(false);
        setEditing(webhook);
        setName(webhook.name);
        setUrl("");
        setEvents(webhook.events);
        setEnabled(webhook.enabled);
        setError("");
    };

    const toggleEvent = (event: DiscordWebhookEvent) => {
        setEvents((current) => current.includes(event)
            ? current.filter((candidate) => candidate !== event)
            : [...current, event]);
    };

    const save = async (event: FormEvent) => {
        event.preventDefault();
        const common = { name, events, enabled };
        const parsed = editing
            ? UpdateDiscordWebhookSchema.safeParse({ ...common, ...(url.trim() ? { url: url.trim() } : {}) })
            : CreateDiscordWebhookSchema.safeParse({ ...common, url: url.trim() });
        if (!parsed.success) {
            const issue = parsed.error.issues[0];
            setError(`${t("administration.webhooks.validation_error")} · ${issue?.path.join(".") || "webhook"}`);
            return;
        }

        setSaving(true);
        setError("");
        // The URL is write-only. Clear it before awaiting the network response so
        // it can never be rendered again, including when the mutation fails.
        setUrl("");
        const response = editing
            ? await apiService.webhooks.update(editing.id, parsed.data, editing.version)
            : await apiService.webhooks.create(parsed.data as ReturnType<typeof CreateDiscordWebhookSchema.parse>);
        setSaving(false);
        if (!response.success) {
            setError(webhookError(response.error, t("administration.webhooks.save_error"), t("administration.webhooks.conflict")));
            return;
        }
        setWebhooks((current) => [...current.filter((candidate) => candidate.id !== response.data.id), response.data]
            .sort((left, right) => left.name.localeCompare(right.name)));
        toast.success(t(editing ? "administration.webhooks.updated" : "administration.webhooks.created"));
        reset();
    };

    const remove = async (webhook: DiscordWebhook) => {
        if (!window.confirm(t("administration.webhooks.delete_confirm"))) return;
        setError("");
        const response = await apiService.webhooks.delete(webhook.id);
        if (!response.success) {
            setError(webhookError(response.error, t("administration.webhooks.delete_error"), t("administration.webhooks.conflict")));
            return;
        }
        if (editing?.id === webhook.id) reset();
        setWebhooks((current) => current.filter((candidate) => candidate.id !== webhook.id));
        toast.success(t("administration.webhooks.deleted"));
    };

    const formatDate = (value: string | null) => value
        ? new Intl.DateTimeFormat(language, { dateStyle: "medium", timeStyle: "short" }).format(new Date(value))
        : t("common.never");

    return (
        <section className="administration-panel webhook-management" aria-labelledby="webhooks-heading">
            <div className="administration-panel__heading">
                <div><h2 id="webhooks-heading">{t("administration.webhooks.title")}</h2><p>{t("administration.webhooks.description")}</p></div>
                {!creating && !editing && <Button type="button" onClick={beginCreate} disabled={webhooks.length >= 16} icon={<Plus size={18} />}>{t("administration.webhooks.create")}</Button>}
            </div>

            {error && <div className="administration-alert administration-alert--error" role="alert">{error}</div>}
            {webhooks.length >= 16 && <div className="administration-notice" role="note">{t("administration.webhooks.limit")}</div>}

            {(creating || editing) && <form className="card administration-editor webhook-editor" onSubmit={save} noValidate>
                <div className="administration-editor__header"><div><h3>{t(creating ? "administration.webhooks.create" : "administration.webhooks.edit")}</h3><p>{t("administration.webhooks.write_only")}</p></div>{editing && <span className="badge badge--info">v{editing.version}</span>}</div>
                <div className="administration-form-grid">
                    <div className="form-group"><label htmlFor="webhook-name">{t("administration.webhooks.name")}</label><input id="webhook-name" className="input" value={name} required maxLength={64} onChange={(change) => setName(change.target.value)} /></div>
                    <div className="form-group"><label htmlFor="webhook-url">{t(editing ? "administration.webhooks.new_url" : "administration.webhooks.url")}</label><input id="webhook-url" className="input" type="password" value={url} required={creating} maxLength={2_048} autoComplete="off" spellCheck={false} placeholder="https://discord.com/api/webhooks/…" onChange={(change) => setUrl(change.target.value)} /><small>{editing ? t("administration.webhooks.keep_url") : t("administration.webhooks.url_hint")}</small></div>
                </div>
                <fieldset className="webhook-events"><legend>{t("administration.webhooks.events")}</legend><div className="webhook-events__grid">{DISCORD_WEBHOOK_EVENTS.map((eventName) => <label key={eventName} className="webhook-event"><input type="checkbox" checked={events.includes(eventName)} onChange={() => toggleEvent(eventName)} /><span><strong>{t(`administration.webhooks.event_labels.${eventName.replaceAll(".", "_")}`)}</strong><code>{eventName}</code></span></label>)}</div></fieldset>
                <label className="administration-toggle"><input type="checkbox" checked={enabled} onChange={(change) => setEnabled(change.target.checked)} /><span><strong>{t("administration.webhooks.enabled")}</strong><small>{t("administration.webhooks.enabled_hint")}</small></span></label>
                <div className="administration-editor__footer"><p><ShieldCheck size={15} />{t("administration.webhooks.security_hint")}</p><Button type="button" variant="secondary" onClick={reset}>{t("common.cancel")}</Button><Button type="submit" isLoading={saving}>{t("common.save")}</Button></div>
            </form>}

            {loading ? <div className="administration-loading" role="status"><span className="spinner spinner--sm" />{t("common.loading")}</div> : webhooks.length === 0 && !creating ? <div className="card administration-empty"><Webhook size={30} aria-hidden="true" /><p>{t("administration.webhooks.empty")}</p></div> : <ul className="webhook-list">{webhooks.map((webhook) => <li key={webhook.id} className="card webhook-card">
                <div className="webhook-card__heading"><span className="webhook-card__icon"><Webhook size={18} /></span><div><strong>{webhook.name}</strong><span>{t(webhook.configured ? "administration.webhooks.configured" : "administration.webhooks.not_configured")}</span></div><span className={`badge badge--${webhook.enabled ? "success" : "muted"}`}>{t(webhook.enabled ? "common.active" : "common.inactive")}</span></div>
                <div className="webhook-card__events">{webhook.events.map((eventName) => <code key={eventName}>{eventName}</code>)}</div>
                <dl className="webhook-card__status"><div><dt><Clock3 size={14} />{t("administration.webhooks.last_delivery")}</dt><dd>{formatDate(webhook.last_delivery_at)}</dd></div><div><dt><CircleAlert size={14} />{t("administration.webhooks.delivery_status")}</dt><dd className={webhook.last_error_code ? "text-danger" : "text-muted"}>{t(webhook.last_error_code ? "administration.webhooks.delivery_failed" : "administration.webhooks.no_error")}</dd></div></dl>
                <div className="webhook-card__actions"><Button type="button" size="sm" variant="secondary" onClick={() => beginEdit(webhook)} icon={<Pencil size={15} />}>{t("common.edit")}</Button><Button type="button" size="icon" variant="ghost" aria-label={`${t("common.delete")} ${webhook.name}`} onClick={() => void remove(webhook)}><Trash2 size={16} /></Button></div>
            </li>)}</ul>}
        </section>
    );
}
