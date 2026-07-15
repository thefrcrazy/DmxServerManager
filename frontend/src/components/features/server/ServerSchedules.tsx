import { FormEvent, useCallback, useEffect, useMemo, useState } from "react";
import { CalendarClock, Clock3, History, Pencil, Plus, Terminal, Trash2 } from "lucide-react";
import { Button } from "@/components/ui";
import { useLanguage } from "@/contexts/LanguageContext";
import { useToast } from "@/contexts/ToastContext";
import { usePermission } from "@/hooks";
import { CreateScheduleSchema, Schedule, ScheduleAction } from "@/schemas/operations";
import { apiService } from "@/services";

type ActionKind = ScheduleAction["kind"];
type TriggerKind = "interval" | "cron";

interface ActionDefinition { kind: ActionKind; capability: string; permission: string }
const ACTIONS: ActionDefinition[] = [
    { kind: "start", capability: "lifecycle", permission: "server.start" },
    { kind: "stop", capability: "lifecycle", permission: "server.stop" },
    { kind: "restart", capability: "lifecycle", permission: "server.start" },
    { kind: "backup", capability: "backups", permission: "server.backup" },
    { kind: "update", capability: "install", permission: "server.update_game" },
    { kind: "console", capability: "console", permission: "server.console.write" },
];

interface ServerSchedulesProps {
    instanceId: string;
    capabilities: string[];
    refreshSignal: number;
}

function validTimezone(value: string): boolean {
    try { new Intl.DateTimeFormat("en", { timeZone: value }).format(); return true; } catch { return false; }
}

export default function ServerSchedules({ instanceId, capabilities, refreshSignal }: ServerSchedulesProps) {
    const { language, t } = useLanguage();
    const { hasPermission } = usePermission();
    const toast = useToast();
    const availableActions = useMemo(
        () => ACTIONS.filter((action) => capabilities.includes(action.capability) && hasPermission(action.permission)),
        [capabilities, hasPermission],
    );
    const [schedules, setSchedules] = useState<Schedule[]>([]);
    const [editing, setEditing] = useState<Schedule | null>(null);
    const [creating, setCreating] = useState(false);
    const [name, setName] = useState("");
    const [triggerKind, setTriggerKind] = useState<TriggerKind>("interval");
    const [intervalSeconds, setIntervalSeconds] = useState("3600");
    const [cronExpression, setCronExpression] = useState("0 0 * * * *");
    const [timezone, setTimezone] = useState(() => Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC");
    const [actionKind, setActionKind] = useState<ActionKind>(availableActions[0]?.kind ?? "start");
    const [consoleCommand, setConsoleCommand] = useState("");
    const [enabled, setEnabled] = useState(true);
    const [loading, setLoading] = useState(true);
    const [saving, setSaving] = useState(false);
    const [error, setError] = useState("");

    const timezones = useMemo(() => {
        const intl = Intl as typeof Intl & { supportedValuesOf?: (key: "timeZone") => string[] };
        const values = intl.supportedValuesOf?.("timeZone") ?? ["UTC", "Europe/Paris", "America/New_York", "Asia/Tokyo"];
        return values.includes(timezone) ? values : [timezone, ...values];
    }, [timezone]);

    const load = useCallback(async () => {
        setError("");
        const response = await apiService.schedules.list(instanceId);
        setLoading(false);
        if (!response.success) {
            setError(response.error.traceId ? `${response.error.message} · trace ${response.error.traceId}` : response.error.message);
            return;
        }
        setSchedules(response.data);
    }, [instanceId]);

    useEffect(() => { void load(); }, [load, refreshSignal]);
    useEffect(() => {
        if (!availableActions.some((action) => action.kind === actionKind)) setActionKind(availableActions[0]?.kind ?? "start");
    }, [actionKind, availableActions]);

    const reset = () => {
        setEditing(null);
        setCreating(false);
        setName("");
        setTriggerKind("interval");
        setIntervalSeconds("3600");
        setCronExpression("0 0 * * * *");
        setTimezone(Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC");
        setActionKind(availableActions[0]?.kind ?? "start");
        setConsoleCommand("");
        setEnabled(true);
        setError("");
    };

    const edit = (schedule: Schedule) => {
        setEditing(schedule);
        setCreating(false);
        setName(schedule.name);
        setTriggerKind(schedule.trigger.kind);
        if (schedule.trigger.kind === "interval") setIntervalSeconds(String(schedule.trigger.seconds));
        else { setCronExpression(schedule.trigger.expression); setTimezone(schedule.trigger.timezone); }
        setActionKind(schedule.action.kind);
        setConsoleCommand(schedule.action.kind === "console" ? schedule.action.command : "");
        setEnabled(schedule.enabled);
        setError("");
    };

    const save = async (event: FormEvent) => {
        event.preventDefault();
        if (triggerKind === "cron" && !validTimezone(timezone.trim())) {
            setError(t("schedules.invalid_timezone"));
            return;
        }
        const action = actionKind === "console"
            ? { kind: "console" as const, command: consoleCommand }
            : { kind: actionKind } as Exclude<ScheduleAction, { kind: "console" }>;
        const candidate = {
            instance_id: instanceId,
            name,
            trigger: triggerKind === "interval"
                ? { kind: "interval" as const, seconds: Number(intervalSeconds) }
                : { kind: "cron" as const, expression: cronExpression, timezone: timezone.trim() },
            action,
            enabled,
        };
        const parsed = CreateScheduleSchema.safeParse(candidate);
        if (!parsed.success) {
            const issue = parsed.error.issues[0];
            setError(`${t("schedules.validation_error")} · ${issue?.path.join(".") || "schedule"}`);
            return;
        }
        setSaving(true);
        setError("");
        const response = editing
            ? await apiService.schedules.update(editing.id, {
                name: parsed.data.name,
                trigger: parsed.data.trigger,
                action: parsed.data.action,
                enabled: parsed.data.enabled,
            }, editing.version)
            : await apiService.schedules.create(parsed.data);
        setSaving(false);
        if (!response.success) {
            const message = response.error.status === 409 ? t("schedules.conflict") : response.error.message;
            setError(response.error.traceId ? `${message} · trace ${response.error.traceId}` : message);
            return;
        }
        toast.success(t(editing ? "schedules.updated" : "schedules.created"));
        reset();
        await load();
    };

    const remove = async (schedule: Schedule) => {
        if (!window.confirm(t("schedules.delete_confirm"))) return;
        const response = await apiService.schedules.delete(schedule.id);
        if (!response.success) {
            setError(response.error.message);
            return;
        }
        if (editing?.id === schedule.id) reset();
        setSchedules((current) => current.filter((item) => item.id !== schedule.id));
        toast.success(t("schedules.deleted"));
    };

    const formatDate = (value: string | null) => value
        ? new Intl.DateTimeFormat(language, { dateStyle: "medium", timeStyle: "short" }).format(new Date(value))
        : t("common.never");

    return (
        <section className="operations-section server-schedules" aria-labelledby="server-schedules-heading">
            <div className="server-backups__header">
                <div><h2 id="server-schedules-heading">{t("schedules.title")}</h2><p>{t("schedules.description")}</p></div>
                {!creating && !editing && availableActions.length > 0 && <Button type="button" onClick={() => setCreating(true)} icon={<Plus size={17} />}>{t("schedules.create")}</Button>}
            </div>

            {error && <div className="operations-error" role="alert">{error}<Button type="button" size="sm" variant="secondary" onClick={() => void load()}>{t("administration.retry")}</Button></div>}
            {availableActions.length === 0 && <div className="operations-notice" role="note">{t("schedules.no_allowed_action")}</div>}

            {(creating || editing) && <form className="card schedule-editor" onSubmit={save} noValidate>
                <div className="schedule-editor__heading"><h3>{t(editing ? "schedules.edit" : "schedules.create")}</h3>{editing && <span className="badge badge--neutral">v{editing.version}</span>}</div>
                <div className="schedule-form-grid">
                    <div className="form-group"><label htmlFor="schedule-name">{t("schedules.name")}</label><input id="schedule-name" className="input" value={name} maxLength={80} required onChange={(event) => setName(event.target.value)} /></div>
                    <div className="form-group"><label htmlFor="schedule-action">{t("schedules.action")}</label><select id="schedule-action" className="select" value={actionKind} onChange={(event) => setActionKind(event.target.value as ActionKind)}>{availableActions.map((action) => <option key={action.kind} value={action.kind}>{t(`schedules.actions.${action.kind}`)}</option>)}</select></div>
                    <div className="form-group"><label htmlFor="schedule-trigger">{t("schedules.trigger")}</label><select id="schedule-trigger" className="select" value={triggerKind} onChange={(event) => setTriggerKind(event.target.value as TriggerKind)}><option value="interval">{t("schedules.interval")}</option><option value="cron">Cron</option></select></div>
                    {triggerKind === "interval" ? <div className="form-group"><label htmlFor="schedule-interval">{t("schedules.interval_seconds")}</label><input id="schedule-interval" className="input" type="number" min={60} max={31_536_000} step={1} value={intervalSeconds} onChange={(event) => setIntervalSeconds(event.target.value)} /><small>{t("schedules.interval_hint")}</small></div> : <>
                        <div className="form-group"><label htmlFor="schedule-cron">{t("schedules.cron_expression")}</label><input id="schedule-cron" className="input" value={cronExpression} required maxLength={455} onChange={(event) => setCronExpression(event.target.value)} /><small>{t("schedules.cron_hint")}</small></div>
                        <div className="form-group"><label htmlFor="schedule-timezone">{t("schedules.timezone")}</label><input id="schedule-timezone" className="input" list="schedule-timezones" value={timezone} required maxLength={64} onChange={(event) => setTimezone(event.target.value)} /><datalist id="schedule-timezones">{timezones.map((zone) => <option key={zone} value={zone} />)}</datalist></div>
                    </>}
                </div>
                {actionKind === "console" && <div className="form-group"><label htmlFor="schedule-command">{t("schedules.console_command")}</label><input id="schedule-command" className="input" value={consoleCommand} required maxLength={4_096} autoComplete="off" onChange={(event) => setConsoleCommand(event.target.value)} /><small>{t("schedules.console_hint")}</small></div>}
                <label className="administration-toggle"><input type="checkbox" checked={enabled} onChange={(event) => setEnabled(event.target.checked)} /><span><strong>{t("schedules.enabled")}</strong><small>{t("schedules.enabled_hint")}</small></span></label>
                <div className="schedule-editor__footer"><Button type="button" variant="secondary" onClick={reset}>{t("common.cancel")}</Button><Button type="submit" isLoading={saving}>{t("common.save")}</Button></div>
            </form>}

            {loading ? <div className="operations-loading" role="status"><span className="spinner spinner--sm" />{t("common.loading")}</div> : schedules.length === 0 ? <div className="operations-empty"><CalendarClock size={30} aria-hidden="true" /><p>{t("schedules.empty")}</p></div> : <ul className="schedule-list">
                {schedules.map((schedule) => <li className="card schedule-card" key={schedule.id}>
                    <div className="schedule-card__heading"><span className="schedule-card__icon">{schedule.action.kind === "console" ? <Terminal size={18} /> : <CalendarClock size={18} />}</span><div><strong>{schedule.name}</strong><span>{t(`schedules.actions.${schedule.action.kind}`)} · {schedule.trigger.kind === "interval" ? `${schedule.trigger.seconds} s` : `${schedule.trigger.expression} (${schedule.trigger.timezone})`}</span></div><span className={`badge badge--${schedule.enabled ? "success" : "muted"}`}>{t(schedule.enabled ? "common.active" : "common.inactive")}</span></div>
                    <dl className="schedule-card__dates"><div><dt><Clock3 size={14} />{t("schedules.next_run")}</dt><dd>{formatDate(schedule.next_run_at)}</dd></div><div><dt><History size={14} />{t("schedules.last_run")}</dt><dd>{formatDate(schedule.last_run_at)}</dd></div></dl>
                    <div className="schedule-card__actions">{availableActions.some((action) => action.kind === schedule.action.kind) && <Button type="button" variant="secondary" size="sm" onClick={() => edit(schedule)} icon={<Pencil size={15} />}>{t("common.edit")}</Button>}<Button type="button" variant="ghost" size="icon" aria-label={`${t("common.delete")} ${schedule.name}`} onClick={() => void remove(schedule)}><Trash2 size={16} /></Button></div>
                </li>)}
            </ul>}
        </section>
    );
}
