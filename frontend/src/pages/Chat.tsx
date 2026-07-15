import { LoaderCircle, MessageSquareText, Send, Trash2 } from "lucide-react";
import { FormEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui";
import { useAuth } from "@/contexts/AuthContext";
import { useLanguage } from "@/contexts/LanguageContext";
import { usePageTitle } from "@/contexts/PageTitleContext";
import { useToast } from "@/contexts/ToastContext";
import { useGlobalEvents, usePermission } from "@/hooks";
import {
    ChatDeletedEventSchema,
    ChatDraftSchema,
    ChatMessage,
    ChatMessageSchema,
} from "@/schemas/operations";
import { apiService } from "@/services";

const CHAT_EVENTS = ["chat.message_created", "chat.message_deleted"] as const;

function mergeNewest(current: ChatMessage[], message: ChatMessage): ChatMessage[] {
    return [message, ...current.filter((item) => item.id !== message.id)];
}

export default function Chat() {
    const { t, language } = useLanguage();
    const { setPageTitle } = usePageTitle();
    const { user } = useAuth();
    const { hasPermission } = usePermission();
    const toast = useToast();
    const [messages, setMessages] = useState<ChatMessage[]>([]);
    const [nextCursor, setNextCursor] = useState<string | null>(null);
    const [draft, setDraft] = useState("");
    const [loading, setLoading] = useState(true);
    const [loadingOlder, setLoadingOlder] = useState(false);
    const [sending, setSending] = useState(false);
    const [loadError, setLoadError] = useState<string | null>(null);
    const endRef = useRef<HTMLDivElement>(null);
    const canRead = hasPermission("chat.read");
    const canWrite = hasPermission("chat.write");

    useEffect(() => {
        setPageTitle(t("chat.title"), t("chat.subtitle"));
    }, [setPageTitle, t]);

    const reload = useCallback(async () => {
        if (!canRead) return;
        const response = await apiService.chat.list();
        if (!response.success) {
            setLoadError(response.error.message);
            setLoading(false);
            return;
        }
        setMessages(response.data.items);
        setNextCursor(response.data.next_before_id);
        setLoadError(null);
        setLoading(false);
    }, [canRead]);

    useEffect(() => { void reload(); }, [reload]);

    const onEvent = useCallback((event: { type: string; payload: unknown }) => {
        if (event.type === "chat.message_created") {
            const parsed = ChatMessageSchema.safeParse(event.payload);
            if (parsed.success) setMessages((current) => mergeNewest(current, parsed.data));
        } else if (event.type === "chat.message_deleted") {
            const parsed = ChatDeletedEventSchema.safeParse(event.payload);
            if (parsed.success) {
                setMessages((current) => current.map((message) => message.id === parsed.data.id
                    ? { ...message, body: null, deleted_at: parsed.data.deleted_at }
                    : message));
            }
        }
    }, []);
    const { isConnected } = useGlobalEvents({
        enabled: canRead,
        eventTypes: CHAT_EVENTS,
        onEvent,
        onResynchronize: reload,
    });

    const chronologicalMessages = useMemo(() => [...messages].reverse(), [messages]);

    const loadOlder = async () => {
        if (!nextCursor || loadingOlder) return;
        setLoadingOlder(true);
        const response = await apiService.chat.list(nextCursor);
        setLoadingOlder(false);
        if (!response.success) return toast.error(response.error.message);
        setMessages((current) => [
            ...current,
            ...response.data.items.filter((candidate) => !current.some((item) => item.id === candidate.id)),
        ]);
        setNextCursor(response.data.next_before_id);
    };

    const submit = async (event: FormEvent) => {
        event.preventDefault();
        const parsed = ChatDraftSchema.safeParse({ body: draft });
        if (!parsed.success) return toast.error(t("chat.invalid_message"));
        setSending(true);
        const response = await apiService.chat.create(parsed.data.body);
        setSending(false);
        if (!response.success) return toast.error(response.error.message);
        setMessages((current) => mergeNewest(current, response.data));
        setDraft("");
        requestAnimationFrame(() => endRef.current?.scrollIntoView({ behavior: "smooth", block: "end" }));
    };

    const remove = async (message: ChatMessage) => {
        const response = await apiService.chat.remove(message.id);
        if (!response.success) return toast.error(response.error.message);
        const deletedAt = new Date().toISOString();
        setMessages((current) => current.map((item) => item.id === message.id
            ? { ...item, body: null, deleted_at: deletedAt }
            : item));
    };

    if (!canRead) {
        return <div className="operations-access-denied" role="alert">{t("chat.access_denied")}</div>;
    }

    return (
        <section className="collaboration-page" aria-labelledby="chat-heading">
            <div className="operations-status">
                <span className={`connection-dot ${isConnected ? "connection-dot--online" : ""}`} aria-hidden="true" />
                <span>{isConnected ? t("realtime.connected") : t("realtime.reconnecting")}</span>
            </div>
            <div className="chat-panel card">
                <h2 id="chat-heading" className="sr-only">{t("chat.conversation")}</h2>
                {nextCursor && (
                    <Button variant="ghost" size="sm" onClick={() => void loadOlder()} isLoading={loadingOlder}>
                        {t("chat.load_older")}
                    </Button>
                )}
                <div className="chat-messages" role="log" aria-live="polite" aria-relevant="additions text">
                    {loading && <div className="operations-loading"><LoaderCircle className="spinner" aria-hidden="true" />{t("common.loading")}</div>}
                    {loadError && <div className="operations-error" role="alert">{loadError}<Button size="sm" variant="secondary" onClick={() => void reload()}>{t("administration.retry")}</Button></div>}
                    {!loading && !loadError && chronologicalMessages.length === 0 && (
                        <div className="operations-empty"><MessageSquareText aria-hidden="true" /><p>{t("chat.empty")}</p></div>
                    )}
                    {chronologicalMessages.map((message) => {
                        const ownMessage = message.author_user_id === user?.id;
                        const canDelete = canWrite && (ownMessage || user?.role === "owner" || user?.role === "admin");
                        return (
                            <article key={message.id} className={`chat-message ${ownMessage ? "chat-message--own" : ""}`}>
                                <header>
                                    <strong>{message.author_username ?? t("chat.deleted_user")}</strong>
                                    <time dateTime={message.created_at}>{new Date(message.created_at).toLocaleString(language === "fr" ? "fr-FR" : "en-US")}</time>
                                    {canDelete && !message.deleted_at && (
                                        <button type="button" className="icon-action" onClick={() => void remove(message)} aria-label={`${t("chat.delete_message")} ${message.author_username ?? ""}`}>
                                            <Trash2 size={15} aria-hidden="true" />
                                        </button>
                                    )}
                                </header>
                                {message.deleted_at
                                    ? <p className="chat-message__deleted">{t("collaboration.message_deleted")}</p>
                                    : <p className="chat-message__body">{message.body}</p>}
                            </article>
                        );
                    })}
                    <div ref={endRef} />
                </div>
                {canWrite && (
                    <form className="chat-compose" onSubmit={(event) => void submit(event)}>
                        <label htmlFor="chat-message">{t("chat.compose_label")}</label>
                        <div className="chat-compose__row">
                            <textarea
                                id="chat-message"
                                className="input"
                                value={draft}
                                maxLength={4_000}
                                rows={2}
                                placeholder={t("chat.placeholder")}
                                onChange={(event) => setDraft(event.target.value)}
                                onKeyDown={(event) => {
                                    if (event.key === "Enter" && !event.shiftKey) {
                                        event.preventDefault();
                                        event.currentTarget.form?.requestSubmit();
                                    }
                                }}
                            />
                            <Button type="submit" icon={<Send size={17} aria-hidden="true" />} isLoading={sending} disabled={!draft.trim()}>
                                {t("common.send")}
                            </Button>
                        </div>
                        <p className="helper-text">{t("chat.keyboard_hint")}</p>
                    </form>
                )}
            </div>
        </section>
    );
}
