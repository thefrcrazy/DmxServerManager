import { ChangeEvent, FormEvent, useEffect, useMemo, useRef, useState } from "react";
import { Archive, Copy, Download, FolderInput, Gamepad2, Link2, Play, ShieldCheck, Upload, X } from "lucide-react";
import { useNavigate } from "react-router-dom";
import { Button } from "@/components/ui";
import { ProfileSettingsFields } from "@/components/features/server";
import { useAuth } from "@/contexts/AuthContext";
import { useLanguage } from "@/contexts/LanguageContext";
import { usePageTitle } from "@/contexts/PageTitleContext";
import { usePermission } from "@/hooks";
import { GameProfile } from "@/schemas/api";
import { apiService } from "@/services";
import { ImportUploadTask } from "@/services/api/imports.client";
import { formatBytes } from "@/utils/formatters";
import { initialProfileSettings, partitionProfileValues, ProfileValue } from "@/utils/profileSettings";
import { fallbackGameArtwork, gameProfileVisual } from "@/constants/gameProfiles";

type CreationMode = "install" | "zip" | "copy" | "attach";

const MAX_IMPORT_BYTES = 4 * 1024 * 1024 * 1024;

function isZipSignature(bytes: Uint8Array): boolean {
    return bytes.length >= 4
        && bytes[0] === 0x50
        && bytes[1] === 0x4b
        && ((bytes[2] === 0x03 && bytes[3] === 0x04)
            || (bytes[2] === 0x05 && bytes[3] === 0x06)
            || (bytes[2] === 0x07 && bytes[3] === 0x08));
}

function isLegacyMinecraftProfile(profile: GameProfile): boolean {
    return [
        "minecraft-java-vanilla",
        "minecraft-java-paper",
        "minecraft-java-fabric",
        "minecraft-java-forge",
        "minecraft-java-neoforge",
        "minecraft-java-spigot",
        "minecraft-java-purpur",
        "minecraft-java-quilt",
    ].includes(profile.id);
}

export default function CreateServer() {
    const { t } = useLanguage();
    const { user } = useAuth();
    const { hasPermission } = usePermission();
    const { setPageTitle } = usePageTitle();
    const navigate = useNavigate();
    const uploadTask = useRef<ImportUploadTask | null>(null);
    const uploadCancelled = useRef(false);
    const fileInput = useRef<HTMLInputElement>(null);
    const [profiles, setProfiles] = useState<GameProfile[]>([]);
    const [profileId, setProfileId] = useState("");
    const [name, setName] = useState("");
    const [mode, setMode] = useState<CreationMode>("install");
    const [autoStart, setAutoStart] = useState(false);
    const [settings, setSettings] = useState<Record<string, ProfileValue>>({});
    const [secrets, setSecrets] = useState<Record<string, string>>({});
    const [sourcePath, setSourcePath] = useState("");
    const [archive, setArchive] = useState<File | null>(null);
    const [uploadProgress, setUploadProgress] = useState(0);
    const [uploadingArchive, setUploadingArchive] = useState(false);
    const [createdInstanceId, setCreatedInstanceId] = useState<string | null>(null);
    const [isLoading, setIsLoading] = useState(true);
    const [isSubmitting, setIsSubmitting] = useState(false);
    const [error, setError] = useState("");
    const [profileOptions, setProfileOptions] = useState<Record<string, readonly string[]>>({});
    const [catalogLoading, setCatalogLoading] = useState(false);

    const canCreate = hasPermission("server.create");
    const canImport = hasPermission("server.files.write");
    const selectedProfile = useMemo(() => profiles.find((profile) => profile.id === profileId), [profileId, profiles]);
    const profileCanImport = selectedProfile?.kind !== "steam_custom" && selectedProfile?.id !== "minecraft-bedrock";
    const availableModes = useMemo(() => {
        const values: CreationMode[] = ["install"];
        if (canImport && profileCanImport) values.push("zip", "copy");
        if (canImport && profileCanImport && user?.role === "owner") values.push("attach");
        return values;
    }, [canImport, profileCanImport, user?.role]);

    useEffect(() => {
        setPageTitle(t("server_creation.title"), t("server_creation.subtitle"), { to: "/servers" });
        if (!canCreate) {
            setIsLoading(false);
            return;
        }
        void apiService.profiles.getProfiles().then((response) => {
            if (!response.success) {
                setError(response.error.message);
                return;
            }
            const selectableProfiles = response.data.filter((profile) => !isLegacyMinecraftProfile(profile));
            setProfiles(selectableProfiles);
            const first = selectableProfiles[0];
            if (first) {
                setProfileId(first.id);
                setSettings(initialProfileSettings(first));
            }
        }).finally(() => setIsLoading(false));
    }, [canCreate, setPageTitle, t]);

    useEffect(() => {
        if (!availableModes.includes(mode)) setMode("install");
    }, [availableModes, mode]);

    useEffect(() => {
        const profile = selectedProfile;
        const usesVersionCatalog = profile?.id === "minecraft-bedrock"
            || profile?.id === "minecraft-java"
            || profile?.id.startsWith("minecraft-java-");
        if (!profile || !usesVersionCatalog) {
            setProfileOptions({});
            setCatalogLoading(false);
            return;
        }
        let active = true;
        const requestedVersion = typeof settings.version === "string" && settings.version
            ? settings.version
            : undefined;
        const selectedLoader = profile.id === "minecraft-java" && typeof settings.loader === "string"
            ? settings.loader
            : undefined;
        const loaderNeedsVersion = selectedLoader
            ? ["fabric", "forge", "neoforge", "purpur", "quilt"].includes(selectedLoader)
            : Boolean(profile.settings_schema.properties.loader_version);
        setProfileOptions({
            version: [],
            ...(loaderNeedsVersion
                ? { loader_version: [] }
                : {}),
        });
        setCatalogLoading(true);
        void apiService.profiles.getVersionCatalog(profile.id, requestedVersion, selectedLoader).then((response) => {
            if (!active) return;
            if (!response.success) {
                // Keep creation usable if an upstream catalog is temporarily
                // unavailable; the normal validated text fields remain as a
                // manual fallback.
                setProfileOptions({});
                return;
            }
            setProfileOptions({
                version: response.data.game_versions,
                ...(loaderNeedsVersion
                    ? { loader_version: response.data.loader_versions }
                    : {}),
            });
            setSettings((current) => {
                const next = { ...current };
                const selectedVersion = response.data.selected_game_version;
                if (selectedVersion && !response.data.game_versions.includes(String(current.version ?? ""))) {
                    next.version = selectedVersion;
                }
                if (loaderNeedsVersion) {
                    const currentLoader = String(current.loader_version ?? "");
                    if (!response.data.loader_versions.includes(currentLoader)) {
                        next.loader_version = response.data.loader_versions[0] ?? "";
                    }
                } else {
                    delete next.loader_version;
                }
                return next;
            });
        }).finally(() => {
            if (active) setCatalogLoading(false);
        });
        return () => {
            active = false;
        };
    }, [selectedProfile, settings.loader, settings.version]);

    useEffect(() => () => uploadTask.current?.cancel(), []);

    const translatedError = (value: string): string => {
        const translated = t(value);
        return translated === value ? value : translated;
    };

    const selectProfile = (id: string) => {
        const profile = profiles.find((candidate) => candidate.id === id);
        if (!profile || createdInstanceId) return;
        setProfileId(id);
        setSettings(initialProfileSettings(profile));
        setSecrets({});
        setArchive(null);
        setSourcePath("");
        setUploadProgress(0);
        setProfileOptions({});
        setError("");
    };

    const selectArchive = async (event: ChangeEvent<HTMLInputElement>) => {
        const selected = event.target.files?.[0] ?? null;
        event.target.value = "";
        if (!selected) return;
        if (selected.size === 0 || selected.size > MAX_IMPORT_BYTES) {
            setError(t("server_creation.archive_size_invalid"));
            return;
        }
        const signature = new Uint8Array(await selected.slice(0, 4).arrayBuffer());
        if (!selected.name.toLowerCase().endsWith(".zip") || !isZipSignature(signature)) {
            setError(t("server_creation.archive_invalid"));
            return;
        }
        setArchive(selected);
        setUploadProgress(0);
        setError("");
    };

    const createInstance = async (): Promise<string | null> => {
        if (createdInstanceId || !selectedProfile) return createdInstanceId;
        const values = partitionProfileValues(selectedProfile, { ...settings, ...secrets });
        const response = await apiService.servers.createServer({
            name: name.trim(),
            profile_id: selectedProfile.id,
            settings: values.settings,
            ...(Object.keys(values.secrets).length > 0 ? { secrets: values.secrets } : {}),
            auto_start: autoStart,
        });
        if (!response.success) {
            setError(translatedError(response.error.message));
            return null;
        }
        setCreatedInstanceId(response.data.id);
        return response.data.id;
    };

    const queueSelectedOperation = async (instanceId: string) => {
        if (mode === "install") {
            if (!selectedProfile?.capabilities.includes("install")) {
                navigate(`/servers/${instanceId}`);
                return;
            }
            const response = await apiService.servers.runAction(instanceId, "install");
            if (!response.success) {
                setError(translatedError(response.error.message));
                return;
            }
            navigate(`/activity?tab=operations&focus=${encodeURIComponent(response.data.id)}&instance=${encodeURIComponent(instanceId)}`);
            return;
        }

        const idempotencyKey = crypto.randomUUID();
        if (mode === "zip") {
            if (!archive) {
                setError(t("server_creation.archive_required"));
                return;
            }
            uploadCancelled.current = false;
            setUploadingArchive(true);
            const task = apiService.imports.uploadZip(instanceId, archive, {
                idempotencyKey,
                onProgress: (progress) => setUploadProgress(progress.percent),
            });
            uploadTask.current = task;
            const response = await task.response;
            uploadTask.current = null;
            setUploadingArchive(false);
            if (uploadCancelled.current) return;
            if (!response.success) {
                setError(translatedError(response.error.message));
                return;
            }
            navigate(`/activity?tab=operations&focus=${encodeURIComponent(response.data.id)}&instance=${encodeURIComponent(instanceId)}`);
            return;
        }

        const response = mode === "attach"
            ? await apiService.imports.attach(instanceId, sourcePath.trim(), idempotencyKey)
            : await apiService.imports.copy(instanceId, sourcePath.trim(), idempotencyKey);
        if (!response.success) {
            setError(translatedError(response.error.message));
            return;
        }
        navigate(`/activity?tab=operations&focus=${encodeURIComponent(response.data.id)}&instance=${encodeURIComponent(instanceId)}`);
    };

    const handleSubmit = async (event: FormEvent) => {
        event.preventDefault();
        if (!selectedProfile || isSubmitting) return;
        if (selectedProfile.settings_schema.required.includes("eula_accepted") && settings.eula_accepted !== true) {
            setError(t("server_creation.eula_required"));
            return;
        }
        if ((mode === "copy" || mode === "attach") && !sourcePath.trim()) {
            setError(t("server_creation.source_required"));
            return;
        }
        if (mode === "zip" && !archive) {
            setError(t("server_creation.archive_required"));
            return;
        }
        setError("");
        setIsSubmitting(true);
        const instanceId = await createInstance();
        if (instanceId) await queueSelectedOperation(instanceId);
        setIsSubmitting(false);
    };

    const cancelUpload = () => {
        uploadCancelled.current = true;
        uploadTask.current?.cancel();
        uploadTask.current = null;
        setIsSubmitting(false);
        setUploadProgress(0);
        setUploadingArchive(false);
        setError(t("server_creation.upload_cancelled"));
    };

    if (isLoading) return <div className="loading-screen"><div className="spinner" /></div>;
    if (!canCreate) return <div className="operations-access-denied" role="alert">{t("server_creation.access_denied")}</div>;

    const modeDetails: Record<CreationMode, { icon: typeof Download; label: string; description: string }> = {
        install: { icon: Download, label: t("server_creation.modes.install"), description: t("server_creation.modes.install_description") },
        zip: { icon: Archive, label: t("server_creation.modes.zip"), description: t("server_creation.modes.zip_description") },
        copy: { icon: Copy, label: t("server_creation.modes.copy"), description: t("server_creation.modes.copy_description") },
        attach: { icon: Link2, label: t("server_creation.modes.attach"), description: t("server_creation.modes.attach_description") },
    };

    return (
        <div className="create-server-page">
            <div className="creation-mode-tabs" role="group" aria-label={t("server_creation.mode_label")}>
                {availableModes.map((value) => {
                    const details = modeDetails[value];
                    const Icon = details.icon;
                    return (
                        <button
                            key={value}
                            type="button"
                            className={`creation-mode-btn ${mode === value ? "creation-mode-btn--active" : ""}`}
                            aria-pressed={mode === value}
                            disabled={Boolean(createdInstanceId) || isSubmitting}
                            onClick={() => { setMode(value); setError(""); }}
                        >
                            <Icon size={21} aria-hidden="true" />
                            <span>{details.label}</span>
                            <small>{details.description}</small>
                        </button>
                    );
                })}
            </div>

            <form onSubmit={(event) => void handleSubmit(event)}>
                <section className="card server-config-card">
                    <h2 className="server-config-title"><Gamepad2 size={20} aria-hidden="true" />{t("server_creation.profile_title")}</h2>
                    <fieldset className="server-creation-fieldset" disabled={Boolean(createdInstanceId) || isSubmitting}>
                        <div className="form-group">
                            <label className="profile-picker__label" htmlFor="server-profile">{t("server_creation.profile_label")}</label>
                            <select
                                id="server-profile"
                                className="sr-only"
                                value={profileId}
                                onChange={(event) => selectProfile(event.target.value)}
                                required
                            >
                                {profiles.map((profile) => (
                                    <option key={`${profile.id}@${profile.revision}`} value={profile.id}>{profile.name}</option>
                                ))}
                            </select>
                            <div className="profile-picker" role="radiogroup" aria-label={t("server_creation.profile_picker_aria")}>
                                {profiles.map((profile) => {
                                    const visual = gameProfileVisual(profile.id, profile.name);
                                    const selected = profile.id === profileId;
                                    return (
                                        <button
                                            key={`${profile.id}@${profile.revision}`}
                                            type="button"
                                            className={`profile-picker__option ${selected ? "profile-picker__option--selected" : ""}`}
                                            role="radio"
                                            aria-checked={selected}
                                            onClick={() => selectProfile(profile.id)}
                                        >
                                            <span className="profile-picker__artwork">
                                                <img
                                                    src={visual.artwork}
                                                    alt=""
                                                    loading="lazy"
                                                    referrerPolicy="no-referrer"
                                                    style={{ objectPosition: visual.artworkPosition }}
                                                    onError={(event) => fallbackGameArtwork(event, visual.fallbackArtwork)}
                                                />
                                            </span>
                                            <span className="profile-picker__content">
                                                <strong>{visual.label}</strong>
                                                <small>{profile.description}</small>
                                            </span>
                                        </button>
                                    );
                                })}
                            </div>
                        </div>
                        <div className="form-group">
                            <label htmlFor="server-name">{t("servers.server_name")}</label>
                            <input id="server-name" className="input" value={name} onChange={(event) => setName(event.target.value)} minLength={1} maxLength={80} required autoFocus />
                        </div>
                        {selectedProfile && (
                            <div className="server-settings-grid">
                                <ProfileSettingsFields
                                    profile={selectedProfile}
                                    values={{ ...settings, ...secrets }}
                                    options={profileOptions}
                                    loadingOptions={catalogLoading ? ["version", "loader_version"] : []}
                                    onChange={(key, value, secret) => secret
                                        ? setSecrets((current) => ({ ...current, [key]: String(value) }))
                                        : setSettings((current) => ({ ...current, [key]: value }))}
                                />
                            </div>
                        )}
                        <label className="form-checkbox">
                            <input type="checkbox" checked={autoStart} onChange={(event) => setAutoStart(event.target.checked)} />
                            <span>{t("server_creation.auto_start")}</span>
                        </label>
                        {selectedProfile?.kind === "steam_custom" && (
                            <div className="advanced-defaults"><ShieldCheck size={16} aria-hidden="true" /><span>{t("servers.steam_profile_locked")}</span></div>
                        )}
                    </fieldset>

                    {mode === "zip" && (
                        <div className="server-import-section">
                            <h3><Archive size={18} aria-hidden="true" />{t("server_creation.archive_title")}</h3>
                            <input ref={fileInput} className="sr-only" type="file" accept=".zip,application/zip" aria-label={t("server_creation.archive_label")} onChange={(event) => void selectArchive(event)} />
                            <button type="button" className={`zip-upload-zone ${archive ? "zip-upload-zone--active" : ""}`} disabled={isSubmitting} onClick={() => fileInput.current?.click()}>
                                {archive
                                    ? <><Archive className="zip-upload-file-icon" aria-hidden="true" /><span className="zip-upload-file-name">{archive.name}</span><small>{formatBytes(archive.size)}</small></>
                                    : <><Upload className="zip-upload-icon" aria-hidden="true" /><span className="zip-upload-text">{t("server_creation.choose_archive")}</span><small>{t("server_creation.archive_hint")}</small></>}
                            </button>
                            {uploadingArchive && (
                                <div className="server-import-progress">
                                    <progress max={100} value={uploadProgress} aria-label={t("server_creation.upload_progress")} />
                                    <span>{Math.round(uploadProgress)} %</span>
                                    <Button type="button" size="sm" variant="secondary" icon={<X size={15} aria-hidden="true" />} onClick={cancelUpload}>{t("common.cancel")}</Button>
                                </div>
                            )}
                        </div>
                    )}

                    {(mode === "copy" || mode === "attach") && (
                        <div className="server-import-section">
                            <h3><FolderInput size={18} aria-hidden="true" />{t(mode === "attach" ? "server_creation.attach_title" : "server_creation.copy_title")}</h3>
                            <div className="form-group">
                                <label htmlFor="server-import-source">{t("server_creation.source_path")}</label>
                                <input id="server-import-source" className="input" value={sourcePath} disabled={isSubmitting} maxLength={4096} autoComplete="off" spellCheck={false} required onChange={(event) => setSourcePath(event.target.value)} />
                                <p className="helper-text">{t(mode === "attach" ? "server_creation.attach_hint" : "server_creation.copy_hint")}</p>
                            </div>
                        </div>
                    )}

                    {createdInstanceId && <div className="operations-notice">{t("server_creation.instance_created_retry")}</div>}
                    {error && <div className="error-banner" role="alert">{error}</div>}
                    <div className="form-footer">
                        <Button type="button" variant="secondary" onClick={() => navigate(createdInstanceId ? `/servers/${createdInstanceId}` : "/servers")}>{t("common.cancel")}</Button>
                        <Button type="submit" size="lg" isLoading={isSubmitting} disabled={!selectedProfile || (mode === "zip" && !archive) || ((mode === "copy" || mode === "attach") && !sourcePath.trim())} icon={<Play size={18} aria-hidden="true" />}>
                            {t(createdInstanceId ? "server_creation.retry_operation" : `server_creation.submit.${mode}`)}
                        </Button>
                    </div>
                </section>
            </form>
        </div>
    );
}
