import { Bell, Check, CheckCheck, LoaderCircle } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui";
import { useLanguage } from "@/contexts/LanguageContext";
import { usePageTitle } from "@/contexts/PageTitleContext";
import { useToast } from "@/contexts/ToastContext";
import { useGlobalEvents, usePermission } from "@/hooks";
import {
    Notification,
    NotificationReadEventSchema,
    NotificationSchema,
    NotificationsReadAllEventSchema,
} from "@/schemas/operations";
import { apiService } from "@/services";

const NOTIFICATION_EVENTS = ["notification.created", "notification.read", "notification.read_all"] as const;

function notificationText(notification: Notification, t: (key: string) => string): string {
    const known = new Set([
        "notifications.job_succeeded",
        "notifications.job_failed",
        "notifications.job_cancelled",
        "notifications.backup_created",
        "notifications.backup_failed",
        "notifications.backup_restored",
        "notifications.backup_restore_failed",
    ]);
    return known.has(notification.message_key) ? t(notification.message_key) : t("notifications.generic");
}

export default function Notifications() {
    const { t, language } = useLanguage();
    const { setPageTitle } = usePageTitle();
    const { hasPermission } = usePermission();
    const toast = useToast();
    const [items, setItems] = useState<Notification[]>([]);
    const [nextCursor, setNextCursor] = useState<string | null>(null);
    const [unreadCount, setUnreadCount] = useState(0);
    const [unreadOnly, setUnreadOnly] = useState(false);
    const [loading, setLoading] = useState(true);
    const [loadingOlder, setLoadingOlder] = useState(false);
    const [loadError, setLoadError] = useState<string | null>(null);
    const knownIds = useRef(new Set<string>());
    const readIds = useRef(new Set<string>());
    const canRead = hasPermission("notifications.read");

    useEffect(() => {
        setPageTitle(t("notifications.title"), t("notifications.subtitle"));
    }, [setPageTitle, t]);

    const reload = useCallback(async () => {
        if (!canRead) return;
        const response = await apiService.notifications.list({ unreadOnly });
        if (!response.success) {
            setLoadError(response.error.message);
            setLoading(false);
            return;
        }
        setItems(response.data.items);
        knownIds.current = new Set(response.data.items.map((item) => item.id));
        readIds.current = new Set(response.data.items.filter((item) => item.read_at).map((item) => item.id));
        setNextCursor(response.data.next_before_id);
        setUnreadCount(response.data.unread_count);
        setLoadError(null);
        setLoading(false);
    }, [canRead, unreadOnly]);

    useEffect(() => {
        setLoading(true);
        void reload();
    }, [reload]);

    const onEvent = useCallback((event: { type: string; payload: unknown }) => {
        if (event.type === "notification.created") {
            const parsed = NotificationSchema.safeParse(event.payload);
            if (!parsed.success) return;
            if (knownIds.current.has(parsed.data.id)) return;
            knownIds.current.add(parsed.data.id);
            setItems((current) => [parsed.data, ...current.filter((item) => item.id !== parsed.data.id)]);
            setUnreadCount((count) => count + 1);
        } else if (event.type === "notification.read") {
            const parsed = NotificationReadEventSchema.safeParse(event.payload);
            if (!parsed.success) return;
            const alreadyRead = readIds.current.has(parsed.data.id);
            readIds.current.add(parsed.data.id);
            setItems((current) => unreadOnly
                ? current.filter((item) => item.id !== parsed.data.id)
                : current.map((item) => item.id === parsed.data.id ? { ...item, read_at: parsed.data.read_at } : item));
            if (!alreadyRead) setUnreadCount((count) => Math.max(0, count - 1));
        } else if (event.type === "notification.read_all") {
            const parsed = NotificationsReadAllEventSchema.safeParse(event.payload);
            if (!parsed.success) return;
            setItems((current) => unreadOnly ? [] : current.map((item) => ({ ...item, read_at: item.read_at ?? parsed.data.read_at })));
            setUnreadCount(0);
        }
    }, [unreadOnly]);
    const { isConnected } = useGlobalEvents({
        enabled: canRead,
        eventTypes: NOTIFICATION_EVENTS,
        onEvent,
        onResynchronize: reload,
    });

    const loadOlder = async () => {
        if (!nextCursor || loadingOlder) return;
        setLoadingOlder(true);
        const response = await apiService.notifications.list({ beforeId: nextCursor, unreadOnly });
        setLoadingOlder(false);
        if (!response.success) return toast.error(response.error.message);
        setItems((current) => [
            ...current,
            ...response.data.items.filter((candidate) => !current.some((item) => item.id === candidate.id)),
        ]);
        setNextCursor(response.data.next_before_id);
        setUnreadCount(response.data.unread_count);
    };

    const markRead = async (notification: Notification) => {
        if (notification.read_at) return;
        const response = await apiService.notifications.markRead(notification.id);
        if (!response.success) return toast.error(response.error.message);
        const readAt = new Date().toISOString();
        const alreadyRead = readIds.current.has(notification.id);
        readIds.current.add(notification.id);
        setItems((current) => unreadOnly
            ? current.filter((item) => item.id !== notification.id)
            : current.map((item) => item.id === notification.id ? { ...item, read_at: readAt } : item));
        if (!alreadyRead) setUnreadCount((count) => Math.max(0, count - 1));
    };

    const markAllRead = async () => {
        const response = await apiService.notifications.markAllRead();
        if (!response.success) return toast.error(response.error.message);
        const readAt = new Date().toISOString();
        for (const item of items) readIds.current.add(item.id);
        setItems((current) => unreadOnly ? [] : current.map((item) => ({ ...item, read_at: item.read_at ?? readAt })));
        setUnreadCount(0);
    };

    if (!canRead) return <div className="operations-access-denied" role="alert">{t("notifications.access_denied")}</div>;

    return (
        <section className="notifications-page" aria-labelledby="notifications-heading">
            <div className="operations-toolbar card">
                <div>
                    <h2 id="notifications-heading">{t("notifications.center")}</h2>
                    <p>{unreadCount} {t("notifications.unread")}</p>
                </div>
                <div className="operations-toolbar__actions">
                    <span className="operations-status">
                        <span className={`connection-dot ${isConnected ? "connection-dot--online" : ""}`} aria-hidden="true" />
                        {isConnected ? t("realtime.connected") : t("realtime.reconnecting")}
                    </span>
                    <label className="form-checkbox">
                        <input type="checkbox" checked={unreadOnly} onChange={(event) => setUnreadOnly(event.target.checked)} />
                        <span>{t("notifications.unread_only")}</span>
                    </label>
                    <Button variant="secondary" size="sm" icon={<CheckCheck size={16} aria-hidden="true" />} onClick={() => void markAllRead()} disabled={unreadCount === 0}>
                        {t("notifications.mark_all_read")}
                    </Button>
                </div>
            </div>
            <div className="notification-list" aria-live="polite">
                {loading && <div className="operations-loading card"><LoaderCircle className="spinner" aria-hidden="true" />{t("common.loading")}</div>}
                {loadError && <div className="operations-error card" role="alert">{loadError}<Button size="sm" variant="secondary" onClick={() => void reload()}>{t("administration.retry")}</Button></div>}
                {!loading && !loadError && items.length === 0 && (
                    <div className="operations-empty card"><Bell aria-hidden="true" /><p>{t("notifications.empty")}</p></div>
                )}
                {items.map((notification) => (
                    <article key={notification.id} className={`notification-card card ${notification.read_at ? "notification-card--read" : ""}`}>
                        <div className={`notification-card__icon notification-card__icon--${notification.kind.includes("failed") ? "danger" : "info"}`}>
                            <Bell size={18} aria-hidden="true" />
                        </div>
                        <div className="notification-card__body">
                            <strong>{notificationText(notification, t)}</strong>
                            <p>{typeof notification.data.action === "string" ? notification.data.action : notification.kind}</p>
                            <time dateTime={notification.created_at}>{new Date(notification.created_at).toLocaleString(language === "fr" ? "fr-FR" : "en-US")}</time>
                        </div>
                        {!notification.read_at && (
                            <Button size="sm" variant="ghost" icon={<Check size={15} aria-hidden="true" />} onClick={() => void markRead(notification)}>
                                {t("notifications.mark_read")}
                            </Button>
                        )}
                    </article>
                ))}
                {nextCursor && <Button variant="ghost" onClick={() => void loadOlder()} isLoading={loadingOlder}>{t("notifications.load_older")}</Button>}
            </div>
        </section>
    );
}
