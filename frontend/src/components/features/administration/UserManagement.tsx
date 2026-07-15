import { FormEvent, useEffect, useMemo, useState } from "react";
import {
    CheckCircle2,
    KeyRound,
    Pencil,
    Plus,
    Server,
    ShieldCheck,
    Trash2,
    UserRound,
    UserRoundPlus,
} from "lucide-react";
import { useLanguage } from "@/contexts/LanguageContext";
import { useToast } from "@/contexts/ToastContext";
import {
    Instance,
    InstanceGrant,
    InstancePermissionId,
    InstancePermissionIdSchema,
    ManagedRole,
    ManagedUser,
    PermissionDescription,
} from "@/schemas/api";
import { apiService } from "@/services";
import { roleLabel, translatedError } from "@/utils/roles";
import PermissionChecklist from "./PermissionChecklist";

interface UserManagementProps {
    users: ManagedUser[];
    roles: ManagedRole[];
    servers: Instance[];
    permissions: PermissionDescription[];
    currentUserId: string;
    currentRole: string;
    canCreate: boolean;
    canUpdate: boolean;
    onUserSaved: (user: ManagedUser) => void;
}

const PRIVILEGED_ROLES = new Set(["owner", "admin"]);

function preferredRole(roles: ManagedRole[]): string {
    return roles.find((role) => role.id === "operator")?.id
        ?? roles.find((role) => role.id !== "owner")?.id
        ?? roles[0]?.id
        ?? "";
}

export default function UserManagement({
    users,
    roles,
    servers,
    permissions,
    currentUserId,
    currentRole,
    canCreate,
    canUpdate,
    onUserSaved,
}: UserManagementProps) {
    const { language, t } = useLanguage();
    const toast = useToast();
    const availableRoles = useMemo(
        () => currentRole === "owner" ? roles : roles.filter((role) => role.id !== "owner"),
        [currentRole, roles],
    );
    const [selectedId, setSelectedId] = useState(users[0]?.id ?? "");
    const [creating, setCreating] = useState(false);
    const [username, setUsername] = useState("");
    const [password, setPassword] = useState("");
    const [roleId, setRoleId] = useState(preferredRole(availableRoles));
    const [userLanguage, setUserLanguage] = useState<"fr" | "en">("fr");
    const [accentColor, setAccentColor] = useState("#3a82f6");
    const [isActive, setIsActive] = useState(true);
    const [saving, setSaving] = useState(false);
    const [error, setError] = useState("");

    const [grants, setGrants] = useState<InstanceGrant[]>([]);
    const [grantsLoading, setGrantsLoading] = useState(false);
    const [grantError, setGrantError] = useState("");
    const [editingGrantId, setEditingGrantId] = useState<string | null>(null);
    const [grantServerId, setGrantServerId] = useState("");
    const [grantUsesRoleDefaults, setGrantUsesRoleDefaults] = useState(true);
    const [grantPermissions, setGrantPermissions] = useState<InstancePermissionId[]>([]);
    const [grantSaving, setGrantSaving] = useState(false);

    const selectedUser = useMemo(
        () => users.find((user) => user.id === selectedId) ?? users[0],
        [selectedId, users],
    );

    useEffect(() => {
        if (creating) return;
        if (!selectedUser) {
            setUsername("");
            setPassword("");
            setRoleId(preferredRole(availableRoles));
            return;
        }
        setSelectedId(selectedUser.id);
        setUsername(selectedUser.username);
        setPassword("");
        setRoleId(selectedUser.role_id);
        setUserLanguage(selectedUser.language);
        setAccentColor(selectedUser.accent_color);
        setIsActive(selectedUser.is_active);
        setError("");
    }, [availableRoles, creating, selectedUser]);

    const selectedUserId = selectedUser?.id;
    const selectedUserRoleId = selectedUser?.role_id;
    useEffect(() => {
        let active = true;
        setEditingGrantId(null);
        setGrantError("");
        if (creating || !selectedUserId || !selectedUserRoleId || PRIVILEGED_ROLES.has(selectedUserRoleId)) {
            setGrants([]);
            setGrantsLoading(false);
            return () => { active = false; };
        }
        setGrantsLoading(true);
        void apiService.admin.listGrants(selectedUserId).then((response) => {
            if (!active) return;
            if (response.success) setGrants(response.data);
            else setGrantError(translatedError(response.error, t("administration.assignments.load_error"), t));
        }).finally(() => {
            if (active) setGrantsLoading(false);
        });
        return () => { active = false; };
    }, [creating, selectedUserId, selectedUserRoleId, t]);

    const selectedRole = roles.find((role) => role.id === selectedUser?.role_id);
    const assignmentCatalog = useMemo(() => {
        if (!selectedRole) return [];
        const wildcard = selectedRole.permissions.includes("*");
        return permissions.filter((permission) => permission.instance_scoped
            && (wildcard || selectedRole.permissions.includes(permission.id)));
    }, [permissions, selectedRole]);

    const availableGrantServers = servers.filter((server) => (
        !grants.some((grant) => grant.instance_id === server.id)
        || editingGrantId === server.id
    ));

    const beginCreate = () => {
        setCreating(true);
        setSelectedId("");
        setUsername("");
        setPassword("");
        setRoleId(preferredRole(availableRoles));
        setUserLanguage(language);
        setAccentColor("#3a82f6");
        setIsActive(true);
        setError("");
    };

    const cancelCreate = () => {
        setPassword("");
        setCreating(false);
        setSelectedId(users[0]?.id ?? "");
    };

    const selectUser = (user: ManagedUser) => {
        setCreating(false);
        setSelectedId(user.id);
    };

    const saveUser = async (event: FormEvent) => {
        event.preventDefault();
        if ((creating && !canCreate) || (!creating && (!selectedUser || !canUpdate))) return;
        const submittedPassword = password;
        setPassword("");
        setSaving(true);
        setError("");
        const response = creating
            ? await apiService.admin.createUser({
                username: username.trim(),
                password: submittedPassword,
                role_id: roleId,
                language: userLanguage,
            })
            : await apiService.admin.updateUser(selectedUser!.id, {
                role_id: roleId,
                is_active: isActive,
                language: userLanguage,
                accent_color: accentColor,
                ...(submittedPassword ? { password: submittedPassword } : {}),
            });
        setSaving(false);
        if (!response.success) {
            setError(translatedError(response.error, t("common.error"), t));
            return;
        }
        onUserSaved(response.data);
        setCreating(false);
        setSelectedId(response.data.id);
        toast.success(t(creating ? "administration.users.created" : "administration.users.updated"));
    };

    const beginAddGrant = () => {
        const firstServer = servers.find((server) => !grants.some((grant) => grant.instance_id === server.id));
        if (!firstServer) {
            setGrantError(t("administration.assignments.no_server_available"));
            return;
        }
        setEditingGrantId("");
        setGrantServerId(firstServer.id);
        setGrantUsesRoleDefaults(true);
        setGrantPermissions([]);
        setGrantError("");
    };

    const beginEditGrant = (grant: InstanceGrant) => {
        setEditingGrantId(grant.instance_id);
        setGrantServerId(grant.instance_id);
        setGrantUsesRoleDefaults(grant.permissions.length === 0);
        setGrantPermissions(grant.permissions);
        setGrantError("");
    };

    const saveGrant = async (event: FormEvent) => {
        event.preventDefault();
        if (!selectedUser || !grantServerId || !canUpdate) return;
        if (!grantUsesRoleDefaults && grantPermissions.length === 0) {
            setGrantError(t("administration.assignments.choose_permissions"));
            return;
        }
        setGrantSaving(true);
        setGrantError("");
        const response = await apiService.admin.setGrant(
            selectedUser.id,
            grantServerId,
            grantUsesRoleDefaults ? [] : grantPermissions,
        );
        setGrantSaving(false);
        if (!response.success) {
            setGrantError(translatedError(response.error, t("common.error"), t));
            return;
        }
        setGrants((current) => [...current.filter((grant) => grant.instance_id !== response.data.instance_id), response.data]
            .sort((left, right) => left.instance_name.localeCompare(right.instance_name)));
        setEditingGrantId(null);
        toast.success(t("administration.assignments.saved"));
    };

    const removeGrant = async (grant: InstanceGrant) => {
        if (!selectedUser || !canUpdate) return;
        if (!window.confirm(t("administration.assignments.delete_confirm"))) return;
        setGrantSaving(true);
        setGrantError("");
        const response = await apiService.admin.deleteGrant(selectedUser.id, grant.instance_id);
        setGrantSaving(false);
        if (!response.success) {
            setGrantError(translatedError(response.error, t("common.error"), t));
            return;
        }
        setGrants((current) => current.filter((item) => item.instance_id !== grant.instance_id));
        if (editingGrantId === grant.instance_id) setEditingGrantId(null);
        toast.success(t("administration.assignments.deleted"));
    };

    const canEditSelected = creating ? canCreate : canUpdate;
    const privilegedSelected = selectedUser ? PRIVILEGED_ROLES.has(selectedUser.role_id) : false;

    return (
        <section className="administration-panel" aria-labelledby="users-heading">
            <div className="administration-panel__heading">
                <div>
                    <h2 id="users-heading">{t("administration.users.title")}</h2>
                    <p>{t("administration.users.description")}</p>
                </div>
                {canCreate && (
                    <button type="button" className="btn btn--primary" onClick={beginCreate}>
                        <UserRoundPlus size={18} aria-hidden="true" />
                        {t("administration.users.create")}
                    </button>
                )}
            </div>

            <div className="administration-split">
                <ul className="administration-list" aria-label={t("administration.tabs.users")}>
                    {users.length === 0 && <li className="administration-empty">{t("administration.users.empty")}</li>}
                    {users.map((user) => (
                        <li key={user.id}>
                            <button
                                type="button"
                                className={`administration-list__item ${!creating && selectedUser?.id === user.id ? "administration-list__item--active" : ""}`}
                                onClick={() => selectUser(user)}
                            >
                                <span className="administration-list__icon"><UserRound size={18} aria-hidden="true" /></span>
                                <span className="administration-list__content">
                                    <strong>{user.username}</strong>
                                    <span>{roleLabel(user.role_id, user.role_name, t)}</span>
                                </span>
                                <span className={`administration-status administration-status--${user.is_active ? "active" : "inactive"}`}>
                                    {t(user.is_active ? "administration.users.status_active" : "administration.users.status_inactive")}
                                </span>
                            </button>
                        </li>
                    ))}
                </ul>

                <div className="administration-user-detail">
                    {(creating || selectedUser) ? (
                        <form className="card administration-editor" onSubmit={saveUser} aria-labelledby="user-editor-heading">
                            <div className="administration-editor__header">
                                <div>
                                    <h3 id="user-editor-heading">
                                        {t(creating ? "administration.users.create" : "administration.users.edit")}
                                    </h3>
                                    {!creating && selectedUser?.must_change_password && (
                                        <p className="administration-inline-status">
                                            <KeyRound size={14} aria-hidden="true" />
                                            {t("administration.users.must_change_password")}
                                        </p>
                                    )}
                                </div>
                            </div>

                            {error && <div className="administration-alert administration-alert--error" role="alert">{error}</div>}

                            <div className="administration-form-grid">
                                <div className="form-group">
                                    <label htmlFor="managed-user-name">{t("administration.users.username")}</label>
                                    <input
                                        id="managed-user-name"
                                        className="form-input"
                                        value={username}
                                        onChange={(event) => setUsername(event.target.value)}
                                        minLength={3}
                                        maxLength={64}
                                        autoComplete="off"
                                        required
                                        disabled={!creating || !canEditSelected}
                                    />
                                </div>

                                <div className="form-group">
                                    <label htmlFor="managed-user-role">{t("administration.users.role")}</label>
                                    <select
                                        id="managed-user-role"
                                        className="form-input"
                                        value={roleId}
                                        onChange={(event) => setRoleId(event.target.value)}
                                        required
                                        disabled={!canEditSelected}
                                    >
                                        {availableRoles.map((role) => (
                                            <option key={role.id} value={role.id}>{roleLabel(role.id, role.name, t)}</option>
                                        ))}
                                    </select>
                                </div>

                                <div className="form-group">
                                    <label htmlFor="managed-user-language">{t("administration.users.language")}</label>
                                    <select
                                        id="managed-user-language"
                                        className="form-input"
                                        value={userLanguage}
                                        onChange={(event) => setUserLanguage(event.target.value as "fr" | "en")}
                                        disabled={!canEditSelected}
                                    >
                                        <option value="fr">Français</option>
                                        <option value="en">English</option>
                                    </select>
                                </div>

                                {!creating && (
                                    <div className="form-group">
                                        <label htmlFor="managed-user-accent">{t("administration.users.accent")}</label>
                                        <div className="administration-color-field">
                                            <input
                                                id="managed-user-accent"
                                                type="color"
                                                value={accentColor}
                                                onChange={(event) => setAccentColor(event.target.value)}
                                                disabled={!canEditSelected}
                                            />
                                            <code>{accentColor}</code>
                                        </div>
                                    </div>
                                )}
                            </div>

                            <div className="form-group">
                                <label htmlFor="managed-user-password">
                                    {t(creating ? "administration.users.password" : "administration.users.reset_password")}
                                </label>
                                <input
                                    id="managed-user-password"
                                    type="password"
                                    className="form-input"
                                    value={password}
                                    onChange={(event) => setPassword(event.target.value)}
                                    minLength={12}
                                    autoComplete="new-password"
                                    required={creating}
                                    disabled={!canEditSelected}
                                />
                                <span className="helper-text">{t("administration.users.password_hint")}</span>
                            </div>

                            {!creating && (
                                <label className="administration-toggle">
                                    <input
                                        type="checkbox"
                                        checked={isActive}
                                        onChange={(event) => setIsActive(event.target.checked)}
                                        disabled={!canEditSelected || selectedUser?.id === currentUserId}
                                    />
                                    <span>
                                        <strong>{t("administration.users.active")}</strong>
                                        <small>{t("administration.users.active_hint")}</small>
                                    </span>
                                </label>
                            )}

                            {canEditSelected && (
                                <div className="administration-editor__footer">
                                    {!creating && selectedUser && (
                                        <p>
                                            {selectedUser.last_login_at
                                                ? `${t("administration.users.last_login")} : ${new Intl.DateTimeFormat(language, { dateStyle: "medium", timeStyle: "short" }).format(new Date(selectedUser.last_login_at))}`
                                                : `${t("administration.users.last_login")} : ${t("administration.users.never_logged_in")}`}
                                        </p>
                                    )}
                                    {creating && (
                                        <button type="button" className="btn btn--secondary" onClick={cancelCreate}>
                                            {t("administration.users.cancel_create")}
                                        </button>
                                    )}
                                    <button type="submit" className="btn btn--primary" disabled={saving || !roleId}>
                                        {saving ? t("common.saving") : t("administration.users.save")}
                                    </button>
                                </div>
                            )}
                        </form>
                    ) : (
                        <div className="card administration-empty">{t("administration.users.select")}</div>
                    )}

                    {!creating && selectedUser && (
                        <section className="card administration-assignments" aria-labelledby="assignments-heading">
                            <div className="administration-assignments__heading">
                                <div>
                                    <h3 id="assignments-heading">{t("administration.assignments.title")}</h3>
                                    <p>{t("administration.assignments.description")}</p>
                                </div>
                                {!privilegedSelected && canUpdate && (
                                    <button
                                        type="button"
                                        className="btn btn--secondary"
                                        onClick={beginAddGrant}
                                        disabled={availableGrantServers.length === 0 || grantsLoading}
                                    >
                                        <Plus size={17} aria-hidden="true" />
                                        {t("administration.assignments.add")}
                                    </button>
                                )}
                            </div>

                            {privilegedSelected ? (
                                <div className="administration-notice">
                                    <ShieldCheck size={19} aria-hidden="true" />
                                    <span>{t("administration.assignments.privileged_role")}</span>
                                </div>
                            ) : (
                                <>
                                    {grantError && <div className="administration-alert administration-alert--error" role="alert">{grantError}</div>}
                                    {grantsLoading && <div className="administration-loading"><span className="spinner spinner--sm" />{t("common.loading")}</div>}
                                    {!grantsLoading && grants.length === 0 && editingGrantId === null && (
                                        <p className="administration-empty">{t("administration.assignments.no_grants")}</p>
                                    )}
                                    <div className="assignment-list">
                                        {grants.map((grant) => (
                                            <article className="assignment-card" key={grant.instance_id}>
                                                <span className="assignment-card__icon"><Server size={17} aria-hidden="true" /></span>
                                                <span className="assignment-card__content">
                                                    <strong>{grant.instance_name}</strong>
                                                    <span>
                                                        {grant.permissions.length === 0
                                                            ? t("administration.assignments.all_role_permissions")
                                                            : grant.permissions.join(", ")}
                                                    </span>
                                                </span>
                                                {canUpdate && (
                                                    <span className="assignment-card__actions">
                                                        <button
                                                            type="button"
                                                            className="btn btn--ghost btn--icon"
                                                            aria-label={`${t("administration.assignments.edit")} — ${grant.instance_name}`}
                                                            onClick={() => beginEditGrant(grant)}
                                                        >
                                                            <Pencil size={16} aria-hidden="true" />
                                                        </button>
                                                        <button
                                                            type="button"
                                                            className="btn btn--danger btn--icon"
                                                            aria-label={`${t("administration.assignments.remove")} — ${grant.instance_name}`}
                                                            onClick={() => removeGrant(grant)}
                                                            disabled={grantSaving}
                                                        >
                                                            <Trash2 size={16} aria-hidden="true" />
                                                        </button>
                                                    </span>
                                                )}
                                            </article>
                                        ))}
                                    </div>

                                    {editingGrantId !== null && (
                                        <form className="assignment-editor" onSubmit={saveGrant} aria-labelledby="assignment-editor-heading">
                                            <h4 id="assignment-editor-heading">
                                                {t(editingGrantId ? "administration.assignments.edit" : "administration.assignments.add")}
                                            </h4>
                                            <div className="form-group">
                                                <label htmlFor="assignment-server">{t("administration.assignments.server")}</label>
                                                <select
                                                    id="assignment-server"
                                                    className="form-input"
                                                    value={grantServerId}
                                                    onChange={(event) => setGrantServerId(event.target.value)}
                                                    disabled={Boolean(editingGrantId)}
                                                    required
                                                >
                                                    {availableGrantServers.map((server) => (
                                                        <option key={server.id} value={server.id}>{server.name}</option>
                                                    ))}
                                                </select>
                                            </div>
                                            <label className="administration-toggle">
                                                <input
                                                    type="checkbox"
                                                    checked={grantUsesRoleDefaults}
                                                    onChange={(event) => setGrantUsesRoleDefaults(event.target.checked)}
                                                />
                                                <span>
                                                    <strong>{t("administration.assignments.defaults")}</strong>
                                                    <small>{t("administration.assignments.defaults_hint")}</small>
                                                </span>
                                            </label>
                                            {!grantUsesRoleDefaults && (
                                                <PermissionChecklist
                                                    catalog={assignmentCatalog}
                                                    selected={grantPermissions}
                                                    onChange={(next) => {
                                                        setGrantPermissions(next.filter(
                                                            (permission): permission is InstancePermissionId =>
                                                                InstancePermissionIdSchema.safeParse(permission).success,
                                                        ));
                                                    }}
                                                    legend={t("administration.roles.permissions")}
                                                />
                                            )}
                                            <div className="assignment-editor__footer">
                                                <button type="button" className="btn btn--secondary" onClick={() => setEditingGrantId(null)}>
                                                    {t("common.cancel")}
                                                </button>
                                                <button
                                                    type="submit"
                                                    className="btn btn--primary"
                                                    disabled={grantSaving || !grantServerId || (!grantUsesRoleDefaults && grantPermissions.length === 0)}
                                                >
                                                    <CheckCircle2 size={17} aria-hidden="true" />
                                                    {grantSaving ? t("common.saving") : t("administration.assignments.save")}
                                                </button>
                                            </div>
                                        </form>
                                    )}
                                </>
                            )}
                        </section>
                    )}
                </div>
            </div>
        </section>
    );
}
