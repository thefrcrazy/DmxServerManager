import { FormEvent, useCallback, useEffect, useMemo, useState } from "react";
import { Clock3, History, Plus, ServerCog, Trash2 } from "lucide-react";
import { Button } from "@/components/ui";
import { useLanguage } from "@/contexts/LanguageContext";
import { useToast } from "@/contexts/ToastContext";
import { GameProfile } from "@/schemas/api";
import { CreateSteamProfileSchema, SteamProfileDefinition } from "@/schemas/operations";
import { apiService } from "@/services";
import { ApiClientError } from "@/services/api/base.client";

type ArgumentKind = "literal" | "instance_dir" | "port";
type StopKind = "stdin" | "interrupt" | "terminate";

interface ArgumentDraft { key: string; kind: ArgumentKind; value: string }
interface PortDraft { key: string; name: string; protocol: "tcp" | "udp"; default: string; adjacentTo: string }
interface PathDraft { key: string; value: string }
interface ProfileDraft {
    id: string;
    name: string;
    description: string;
    appId: string;
    branch: string;
    linuxExecutable: string;
    windowsExecutable: string;
    arguments: ArgumentDraft[];
    ports: PortDraft[];
    savePaths: PathDraft[];
    readyPattern: string;
    stopKind: StopKind;
    stopCommand: string;
    stopTimeout: string;
}

const key = () => crypto.randomUUID();
const emptyDraft = (): ProfileDraft => ({
    id: "steam-",
    name: "",
    description: "",
    appId: "",
    branch: "",
    linuxExecutable: "",
    windowsExecutable: "",
    arguments: [],
    ports: [{ key: key(), name: "game", protocol: "udp", default: "27015", adjacentTo: "" }],
    savePaths: [{ key: key(), value: "saves" }],
    readyPattern: "",
    stopKind: "interrupt",
    stopCommand: "stop",
    stopTimeout: "60",
});

function parseArgument(argument: string): ArgumentDraft {
    if (argument === "{{instance_dir}}") return { key: key(), kind: "instance_dir", value: "" };
    const port = /^\{\{port:([^}]+)\}\}$/.exec(argument);
    if (port) return { key: key(), kind: "port", value: port[1] ?? "" };
    return { key: key(), kind: "literal", value: argument };
}

function draftFromProfile(profile: GameProfile): ProfileDraft {
    const steam = profile.steam_profile;
    if (!steam) return emptyDraft();
    return {
        id: profile.id,
        name: profile.name,
        description: profile.description,
        appId: String(steam.app_id),
        branch: steam.branch ?? "",
        linuxExecutable: steam.executable.linux_x86_64 ?? "",
        windowsExecutable: steam.executable.windows_x86_64 ?? "",
        arguments: steam.arguments.map(parseArgument),
        ports: steam.ports.map((port) => ({
            key: key(), name: port.name, protocol: port.protocol, default: String(port.default), adjacentTo: port.adjacent_to ?? "",
        })),
        savePaths: steam.save_paths.map((value) => ({ key: key(), value })),
        readyPattern: steam.ready_log_pattern ?? "",
        stopKind: steam.stop_strategy.kind,
        stopCommand: steam.stop_strategy.kind === "stdin" ? steam.stop_strategy.command : "stop",
        stopTimeout: String(steam.stop_strategy.timeout_seconds),
    };
}

function serializeDefinition(draft: ProfileDraft): SteamProfileDefinition {
    const timeout = Number(draft.stopTimeout);
    const stopStrategy = draft.stopKind === "stdin"
        ? { kind: "stdin" as const, command: draft.stopCommand, timeout_seconds: timeout }
        : { kind: draft.stopKind, timeout_seconds: timeout };
    return {
        name: draft.name.trim(),
        description: draft.description.trim(),
        app_id: Number(draft.appId),
        branch: draft.branch.trim() || null,
        executable: {
            linux_x86_64: draft.linuxExecutable.trim() || null,
            windows_x86_64: draft.windowsExecutable.trim() || null,
        },
        arguments: draft.arguments.map((argument) => argument.kind === "instance_dir"
            ? "{{instance_dir}}"
            : argument.kind === "port" ? `{{port:${argument.value}}}` : argument.value),
        ports: draft.ports.map((port) => ({
            name: port.name.trim(),
            protocol: port.protocol,
            default: Number(port.default),
            adjacent_to: port.adjacentTo || null,
        })),
        save_paths: draft.savePaths.map((path) => path.value.trim()),
        ready_log_pattern: draft.readyPattern.trim() || null,
        stop_strategy: stopStrategy,
    };
}

function typedError(error: ApiClientError, fallback: string, conflict: string): string {
    const message = error.status === 409 ? conflict : (error.code ? `${fallback} (${error.code})` : error.message || fallback);
    return error.traceId ? `${message} · trace ${error.traceId}` : message;
}

export default function SteamProfileManagement() {
    const { t } = useLanguage();
    const toast = useToast();
    const [profiles, setProfiles] = useState<GameProfile[]>([]);
    const [revisions, setRevisions] = useState<GameProfile[]>([]);
    const [selectedId, setSelectedId] = useState("");
    const [creating, setCreating] = useState(false);
    const [draft, setDraft] = useState<ProfileDraft>(emptyDraft);
    const [loading, setLoading] = useState(true);
    const [saving, setSaving] = useState(false);
    const [error, setError] = useState("");

    const selected = useMemo(() => profiles.find((profile) => profile.id === selectedId), [profiles, selectedId]);
    const portNames = [...new Set(draft.ports.map((port) => port.name).filter(Boolean))];

    const loadProfiles = useCallback(async (preferredId?: string) => {
        setLoading(true);
        setError("");
        const response = await apiService.profiles.getProfiles();
        setLoading(false);
        if (!response.success) {
            setError(typedError(response.error, t("administration.steam.load_error"), t("administration.steam.conflict")));
            return;
        }
        const custom = response.data.filter((profile) => profile.kind === "steam_custom" && profile.steam_profile);
        setProfiles(custom);
        setSelectedId((current) => preferredId && custom.some((profile) => profile.id === preferredId)
            ? preferredId
            : custom.some((profile) => profile.id === current) ? current : custom[0]?.id ?? "");
    }, [t]);

    useEffect(() => { void loadProfiles(); }, [loadProfiles]);

    useEffect(() => {
        if (creating || !selected) return;
        setDraft(draftFromProfile(selected));
        setRevisions([]);
        setError("");
        void apiService.profiles.getRevisions(selected.id).then((response) => {
            if (response.success) setRevisions([...response.data].sort((left, right) => right.revision - left.revision));
            else setError(typedError(response.error, t("administration.steam.revisions_error"), t("administration.steam.conflict")));
        });
    }, [creating, selected, t]);

    const beginCreate = () => {
        setCreating(true);
        setSelectedId("");
        setRevisions([]);
        setDraft(emptyDraft());
        setError("");
    };

    const cancelCreate = () => {
        setCreating(false);
        const next = profiles[0];
        setSelectedId(next?.id ?? "");
        if (next) setDraft(draftFromProfile(next));
    };

    const updatePort = (rowKey: string, patch: Partial<PortDraft>) => {
        setDraft((current) => ({ ...current, ports: current.ports.map((port) => port.key === rowKey ? { ...port, ...patch } : port) }));
    };

    const updateArgument = (rowKey: string, patch: Partial<ArgumentDraft>) => {
        setDraft((current) => ({ ...current, arguments: current.arguments.map((argument) => argument.key === rowKey ? { ...argument, ...patch } : argument) }));
    };

    const save = async (event: FormEvent) => {
        event.preventDefault();
        const candidate = { id: draft.id.trim(), definition: serializeDefinition(draft) };
        const parsed = CreateSteamProfileSchema.safeParse(candidate);
        if (!parsed.success) {
            const issue = parsed.error.issues[0];
            setError(`${t("administration.steam.validation_error")} · ${issue?.path.join(".") || "definition"}`);
            return;
        }
        setSaving(true);
        setError("");
        const response = creating
            ? await apiService.profiles.createSteam(parsed.data)
            : await apiService.profiles.reviseSteam(selected!.id, parsed.data.definition, selected!.revision);
        setSaving(false);
        if (!response.success) {
            setError(typedError(response.error, t("administration.steam.save_error"), t("administration.steam.conflict")));
            return;
        }
        setCreating(false);
        await loadProfiles(response.data.id);
        toast.success(t(creating ? "administration.steam.created" : "administration.steam.revised"));
    };

    const remove = async () => {
        if (!selected || !window.confirm(t("administration.steam.delete_confirm"))) return;
        setSaving(true);
        const response = await apiService.profiles.deleteSteam(selected.id);
        setSaving(false);
        if (!response.success) {
            setError(typedError(response.error, t("administration.steam.delete_error"), t("administration.steam.in_use")));
            return;
        }
        await loadProfiles();
        toast.success(t("administration.steam.deleted"));
    };

    return (
        <section className="administration-panel steam-profiles" aria-labelledby="steam-profiles-heading">
            <div className="administration-panel__heading">
                <div>
                    <h2 id="steam-profiles-heading">{t("administration.steam.title")}</h2>
                    <p>{t("administration.steam.description")}</p>
                </div>
                <Button type="button" onClick={beginCreate} icon={<Plus size={18} aria-hidden="true" />}>
                    {t("administration.steam.create")}
                </Button>
            </div>

            {error && <div className="administration-alert administration-alert--error" role="alert">{error}</div>}
            {loading ? <div className="administration-loading" role="status"><span className="spinner spinner--sm" />{t("common.loading")}</div> : (
                <div className="administration-split">
                    <ul className="administration-list" aria-label={t("administration.tabs.steam_profiles") }>
                        {profiles.length === 0 && !creating && <li className="administration-empty">{t("administration.steam.empty")}</li>}
                        {profiles.map((profile) => (
                            <li key={profile.id}>
                                <button type="button" aria-pressed={!creating && selected?.id === profile.id} className={`administration-list__item ${!creating && selected?.id === profile.id ? "administration-list__item--active" : ""}`} onClick={() => { setCreating(false); setSelectedId(profile.id); }}>
                                    <span className="administration-list__icon"><ServerCog size={18} aria-hidden="true" /></span>
                                    <span className="administration-list__content"><strong>{profile.name}</strong><span>{profile.id} · r{profile.revision}</span></span>
                                </button>
                            </li>
                        ))}
                    </ul>

                    {(creating || selected) ? (
                        <form className="card administration-editor steam-profile-editor" onSubmit={save} noValidate>
                            <div className="administration-editor__header">
                                <div><h3>{t(creating ? "administration.steam.create" : "administration.steam.edit")}</h3><p>{t("administration.steam.best_effort")}</p></div>
                                {!creating && <span className="badge badge--info">r{selected?.revision}</span>}
                            </div>

                            <div className="administration-form-grid">
                                <div className="form-group"><label htmlFor="steam-profile-id">{t("administration.steam.id")}</label><input id="steam-profile-id" className="input" value={draft.id} disabled={!creating} required pattern="steam-[a-z0-9-]+" maxLength={64} onChange={(event) => setDraft({ ...draft, id: event.target.value })} /><small>{t("administration.steam.id_hint")}</small></div>
                                <div className="form-group"><label htmlFor="steam-app-id">{t("administration.steam.app_id")}</label><input id="steam-app-id" className="input" type="number" min={1} max={4_294_967_295} value={draft.appId} disabled={!creating} required onChange={(event) => setDraft({ ...draft, appId: event.target.value })} /><small>{t("administration.steam.app_id_immutable")}</small></div>
                                <div className="form-group"><label htmlFor="steam-name">{t("administration.steam.name")}</label><input id="steam-name" className="input" value={draft.name} required maxLength={80} onChange={(event) => setDraft({ ...draft, name: event.target.value })} /></div>
                                <div className="form-group"><label htmlFor="steam-branch">{t("administration.steam.branch")}</label><input id="steam-branch" className="input" value={draft.branch} maxLength={64} pattern="[A-Za-z0-9._-]+" onChange={(event) => setDraft({ ...draft, branch: event.target.value })} /></div>
                            </div>
                            <div className="form-group"><label htmlFor="steam-description">{t("administration.steam.profile_description")}</label><textarea id="steam-description" className="textarea" value={draft.description} required maxLength={500} rows={3} onChange={(event) => setDraft({ ...draft, description: event.target.value })} /></div>

                            <fieldset className="steam-fieldset"><legend>{t("administration.steam.executables")}</legend><p>{t("administration.steam.relative_paths")}</p><div className="administration-form-grid">
                                <div className="form-group"><label htmlFor="steam-linux-executable">Linux AMD64</label><input id="steam-linux-executable" className="input" value={draft.linuxExecutable} maxLength={512} placeholder="bin/server" onChange={(event) => setDraft({ ...draft, linuxExecutable: event.target.value })} /></div>
                                <div className="form-group"><label htmlFor="steam-windows-executable">Windows AMD64</label><input id="steam-windows-executable" className="input" value={draft.windowsExecutable} maxLength={512} placeholder="Server.exe" onChange={(event) => setDraft({ ...draft, windowsExecutable: event.target.value })} /></div>
                            </div></fieldset>

                            <fieldset className="steam-fieldset"><legend>{t("administration.steam.arguments")}</legend><p>{t("administration.steam.arguments_hint")}</p><div className="steam-repeat-list">
                                {draft.arguments.map((argument, index) => <div className="steam-repeat-row steam-argument-row" key={argument.key}>
                                    <label className="sr-only" htmlFor={`steam-argument-kind-${argument.key}`}>{t("administration.steam.argument_type")} {index + 1}</label>
                                    <select id={`steam-argument-kind-${argument.key}`} className="select" value={argument.kind} onChange={(event) => updateArgument(argument.key, { kind: event.target.value as ArgumentKind, value: "" })}><option value="literal">{t("administration.steam.argument_literal")}</option><option value="instance_dir">instance_dir</option><option value="port">port</option></select>
                                    {argument.kind === "literal" ? <><label className="sr-only" htmlFor={`steam-argument-value-${argument.key}`}>{t("administration.steam.argument_value")} {index + 1}</label><input id={`steam-argument-value-${argument.key}`} className="input" value={argument.value} maxLength={8_192} onChange={(event) => updateArgument(argument.key, { value: event.target.value })} /></> : argument.kind === "port" ? <><label className="sr-only" htmlFor={`steam-argument-port-${argument.key}`}>{t("administration.steam.argument_port")} {index + 1}</label><select id={`steam-argument-port-${argument.key}`} className="select" value={argument.value} onChange={(event) => updateArgument(argument.key, { value: event.target.value })}><option value="">—</option>{portNames.map((name) => <option key={name} value={name}>{name}</option>)}</select></> : <code>{"{{instance_dir}}"}</code>}
                                    <Button type="button" variant="ghost" size="icon" aria-label={`${t("common.remove")} ${index + 1}`} onClick={() => setDraft({ ...draft, arguments: draft.arguments.filter((row) => row.key !== argument.key) })}><Trash2 size={16} /></Button>
                                </div>)}
                                <Button type="button" variant="secondary" size="sm" onClick={() => setDraft({ ...draft, arguments: [...draft.arguments, { key: key(), kind: "literal", value: "" }] })} icon={<Plus size={16} />}>{t("administration.steam.add_argument")}</Button>
                            </div></fieldset>

                            <fieldset className="steam-fieldset"><legend>{t("administration.steam.ports")}</legend><div className="steam-repeat-list">
                                {draft.ports.map((port, index) => <div className="steam-repeat-row steam-port-row" key={port.key}>
                                    <div className="form-group"><label htmlFor={`steam-port-name-${port.key}`}>{t("administration.steam.port_name")} {index + 1}</label><input id={`steam-port-name-${port.key}`} className="input" value={port.name} pattern="[a-z][a-z0-9_]*" maxLength={32} onChange={(event) => updatePort(port.key, { name: event.target.value })} /></div>
                                    <div className="form-group"><label htmlFor={`steam-port-protocol-${port.key}`}>{t("administration.steam.protocol")}</label><select id={`steam-port-protocol-${port.key}`} className="select" value={port.protocol} onChange={(event) => updatePort(port.key, { protocol: event.target.value as "tcp" | "udp" })}><option value="udp">UDP</option><option value="tcp">TCP</option></select></div>
                                    <div className="form-group"><label htmlFor={`steam-port-default-${port.key}`}>{t("administration.steam.default_port")}</label><input id={`steam-port-default-${port.key}`} className="input" type="number" min={1} max={65_535} value={port.default} onChange={(event) => updatePort(port.key, { default: event.target.value })} /></div>
                                    <div className="form-group"><label htmlFor={`steam-port-adjacent-${port.key}`}>{t("administration.steam.adjacent_to")}</label><select id={`steam-port-adjacent-${port.key}`} className="select" value={port.adjacentTo} onChange={(event) => updatePort(port.key, { adjacentTo: event.target.value })}><option value="">—</option>{portNames.filter((name) => name !== port.name).map((name) => <option key={name} value={name}>{name}</option>)}</select></div>
                                    <Button type="button" variant="ghost" size="icon" aria-label={`${t("common.remove")} ${index + 1}`} disabled={draft.ports.length === 1} onClick={() => setDraft({ ...draft, ports: draft.ports.filter((row) => row.key !== port.key) })}><Trash2 size={16} /></Button>
                                </div>)}
                                <Button type="button" variant="secondary" size="sm" disabled={draft.ports.length >= 16} onClick={() => setDraft({ ...draft, ports: [...draft.ports, { key: key(), name: "", protocol: "udp", default: "27016", adjacentTo: "" }] })} icon={<Plus size={16} />}>{t("administration.steam.add_port")}</Button>
                            </div></fieldset>

                            <fieldset className="steam-fieldset"><legend>{t("administration.steam.save_paths")}</legend><p>{t("administration.steam.save_paths_hint")}</p><div className="steam-repeat-list">
                                {draft.savePaths.map((path, index) => <div className="steam-repeat-row" key={path.key}><label className="sr-only" htmlFor={`steam-save-path-${path.key}`}>{t("administration.steam.save_path")} {index + 1}</label><input id={`steam-save-path-${path.key}`} className="input" value={path.value} maxLength={512} onChange={(event) => setDraft({ ...draft, savePaths: draft.savePaths.map((row) => row.key === path.key ? { ...row, value: event.target.value } : row) })} /><Button type="button" variant="ghost" size="icon" aria-label={`${t("common.remove")} ${index + 1}`} disabled={draft.savePaths.length === 1} onClick={() => setDraft({ ...draft, savePaths: draft.savePaths.filter((row) => row.key !== path.key) })}><Trash2 size={16} /></Button></div>)}
                                <Button type="button" variant="secondary" size="sm" disabled={draft.savePaths.length >= 32} onClick={() => setDraft({ ...draft, savePaths: [...draft.savePaths, { key: key(), value: "" }] })} icon={<Plus size={16} />}>{t("administration.steam.add_save_path")}</Button>
                            </div></fieldset>

                            <fieldset className="steam-fieldset"><legend>{t("administration.steam.lifecycle")}</legend><div className="administration-form-grid">
                                <div className="form-group"><label htmlFor="steam-readiness">{t("administration.steam.readiness")}</label><input id="steam-readiness" className="input" value={draft.readyPattern} maxLength={256} onChange={(event) => setDraft({ ...draft, readyPattern: event.target.value })} /><small>{t("administration.steam.readiness_hint")}</small></div>
                                <div className="form-group"><label htmlFor="steam-stop-kind">{t("administration.steam.stop_strategy")}</label><select id="steam-stop-kind" className="select" value={draft.stopKind} onChange={(event) => setDraft({ ...draft, stopKind: event.target.value as StopKind })}><option value="stdin">stdin</option><option value="interrupt">interrupt</option><option value="terminate">terminate</option></select></div>
                                {draft.stopKind === "stdin" && <div className="form-group"><label htmlFor="steam-stop-command">{t("administration.steam.stop_command")}</label><input id="steam-stop-command" className="input" value={draft.stopCommand} maxLength={256} onChange={(event) => setDraft({ ...draft, stopCommand: event.target.value })} /></div>}
                                <div className="form-group"><label htmlFor="steam-stop-timeout">{t("administration.steam.stop_timeout")}</label><input id="steam-stop-timeout" className="input" type="number" min={1} max={300} value={draft.stopTimeout} onChange={(event) => setDraft({ ...draft, stopTimeout: event.target.value })} /></div>
                            </div></fieldset>

                            {!creating && revisions.length > 0 && <details className="steam-revisions"><summary><History size={16} aria-hidden="true" />{t("administration.steam.revisions")} ({revisions.length})</summary><ol>{revisions.map((revision) => <li key={revision.revision}><span><strong>r{revision.revision}</strong> · AppID {revision.steam_profile?.app_id}</span><span>{revision.platforms.join(", ")} · {revision.ports.length} {t("administration.steam.ports_count")}</span></li>)}</ol></details>}

                            <div className="administration-editor__footer">
                                <p><Clock3 size={14} aria-hidden="true" />{t("administration.steam.revision_hint")}</p>
                                {creating ? <Button type="button" variant="secondary" onClick={cancelCreate}>{t("common.cancel")}</Button> : <Button type="button" variant="danger" onClick={() => void remove()} disabled={saving} icon={<Trash2 size={16} />}>{t("common.delete")}</Button>}
                                <Button type="submit" isLoading={saving}>{t(creating ? "administration.steam.create" : "administration.steam.create_revision")}</Button>
                            </div>
                        </form>
                    ) : <div className="card administration-empty">{t("administration.steam.empty")}</div>}
                </div>
            )}
        </section>
    );
}
