import { FormEvent, useEffect, useMemo, useState } from "react";
import { Pencil, Plus, Shield, Trash2 } from "lucide-react";
import { useLanguage } from "@/contexts/LanguageContext";
import { useToast } from "@/contexts/ToastContext";
import { ManagedRole, PermissionDescription, PermissionId } from "@/schemas/api";
import { apiService } from "@/services";
import { roleLabel, translatedError } from "@/utils/roles";
import PermissionChecklist from "./PermissionChecklist";

interface RoleManagementProps {
    roles: ManagedRole[];
    permissions: PermissionDescription[];
    canManage: boolean;
    onRoleSaved: (role: ManagedRole) => void;
    onRoleDeleted: (id: string) => void;
}

export default function RoleManagement({
    roles,
    permissions,
    canManage,
    onRoleSaved,
    onRoleDeleted,
}: RoleManagementProps) {
    const { t } = useLanguage();
    const toast = useToast();
    const [selectedId, setSelectedId] = useState(roles[0]?.id ?? "");
    const [creating, setCreating] = useState(false);
    const [name, setName] = useState("");
    const [selectedPermissions, setSelectedPermissions] = useState<PermissionId[]>([]);
    const [saving, setSaving] = useState(false);
    const [error, setError] = useState("");

    const selectedRole = useMemo(
        () => roles.find((role) => role.id === selectedId) ?? roles[0],
        [roles, selectedId],
    );

    useEffect(() => {
        if (creating) return;
        if (!selectedRole) {
            setName("");
            setSelectedPermissions([]);
            return;
        }
        setSelectedId(selectedRole.id);
        setName(selectedRole.name);
        setSelectedPermissions(
            selectedRole.permissions.filter(
                (permission): permission is PermissionId => permission !== "*",
            ),
        );
        setError("");
    }, [creating, selectedRole]);

    const beginCreate = () => {
        setCreating(true);
        setSelectedId("");
        setName("");
        setSelectedPermissions([]);
        setError("");
    };

    const selectRole = (role: ManagedRole) => {
        setCreating(false);
        setSelectedId(role.id);
    };

    const save = async (event: FormEvent) => {
        event.preventDefault();
        if (!canManage || (!creating && selectedRole?.is_system)) return;
        setSaving(true);
        setError("");
        const response = creating
            ? await apiService.admin.createRole({ name: name.trim(), permissions: selectedPermissions })
            : await apiService.admin.updateRole(selectedRole!.id, {
                name: name.trim(),
                permissions: selectedPermissions,
            });
        setSaving(false);
        if (!response.success) {
            setError(translatedError(response.error, t("common.error"), t));
            return;
        }
        onRoleSaved(response.data);
        setCreating(false);
        setSelectedId(response.data.id);
        toast.success(t(creating ? "administration.roles.created" : "administration.roles.updated"));
    };

    const remove = async () => {
        if (!selectedRole || selectedRole.is_system || !canManage) return;
        if (!window.confirm(t("administration.roles.delete_confirm"))) return;
        setSaving(true);
        setError("");
        const response = await apiService.admin.deleteRole(selectedRole.id);
        setSaving(false);
        if (!response.success) {
            setError(translatedError(response.error, t("common.error"), t));
            return;
        }
        onRoleDeleted(selectedRole.id);
        setSelectedId(roles.find((role) => role.id !== selectedRole.id)?.id ?? "");
        toast.success(t("administration.roles.deleted"));
    };

    const readOnly = !creating && (selectedRole?.is_system || !canManage);

    return (
        <section className="administration-panel" aria-labelledby="roles-heading">
            <div className="administration-panel__heading">
                <div>
                    <h2 id="roles-heading">{t("administration.roles.title")}</h2>
                    <p>{t("administration.roles.description")}</p>
                </div>
                {canManage && (
                    <button type="button" className="btn btn--primary" onClick={beginCreate}>
                        <Plus size={18} aria-hidden="true" />
                        {t("administration.roles.create")}
                    </button>
                )}
            </div>

            <div className="administration-split">
                <ul className="administration-list" aria-label={t("administration.tabs.roles")}>
                    {roles.map((role) => (
                        <li key={role.id}>
                            <button
                                type="button"
                                className={`administration-list__item ${!creating && selectedRole?.id === role.id ? "administration-list__item--active" : ""}`}
                                onClick={() => selectRole(role)}
                            >
                                <span className="administration-list__icon"><Shield size={18} aria-hidden="true" /></span>
                                <span className="administration-list__content">
                                    <strong>{roleLabel(role.id, role.name, t)}</strong>
                                    <span>{role.permissions.includes("*") ? "*" : role.permissions.length} {t("administration.roles.permissions").toLowerCase()}</span>
                                </span>
                                <span className={`badge ${role.is_system ? "badge--neutral" : "badge--info"}`}>
                                    {t(role.is_system ? "administration.roles.system_badge" : "administration.roles.custom_badge")}
                                </span>
                            </button>
                        </li>
                    ))}
                </ul>

                <form className="card administration-editor" onSubmit={save} aria-labelledby="role-editor-heading">
                    <div className="administration-editor__header">
                        <div>
                            <h3 id="role-editor-heading">
                                {creating ? t("administration.roles.create") : t("administration.roles.edit")}
                            </h3>
                            {readOnly && <p>{t("administration.roles.immutable")}</p>}
                        </div>
                        {!creating && selectedRole && !selectedRole.is_system && canManage && (
                            <button
                                type="button"
                                className="btn btn--danger btn--icon"
                                aria-label={t("common.delete")}
                                onClick={remove}
                                disabled={saving}
                            >
                                <Trash2 size={17} aria-hidden="true" />
                            </button>
                        )}
                    </div>

                    {error && <div className="alert alert--error" role="alert">{error}</div>}

                    <div className="form-group">
                        <label htmlFor="role-name">{t("administration.roles.name")}</label>
                        <input
                            id="role-name"
                            className="form-input"
                            value={name}
                            onChange={(event) => setName(event.target.value)}
                            maxLength={64}
                            required
                            disabled={readOnly}
                        />
                    </div>

                    {selectedRole?.permissions.includes("*") && !creating ? (
                        <div className="permission-wildcard" aria-label={t("administration.roles.permissions")}>
                            <Shield size={18} aria-hidden="true" />
                            <code>*</code>
                        </div>
                    ) : (
                        <PermissionChecklist
                            catalog={permissions}
                            selected={selectedPermissions}
                            onChange={setSelectedPermissions}
                            legend={t("administration.roles.permissions")}
                            disabled={readOnly}
                        />
                    )}

                    {!readOnly && (
                        <div className="administration-editor__footer">
                            {!creating && (
                                <p><Pencil size={14} aria-hidden="true" /> {t("administration.roles.sessions_revoked")}</p>
                            )}
                            {creating && (
                                <button type="button" className="btn btn--secondary" onClick={() => setCreating(false)}>
                                    {t("common.cancel")}
                                </button>
                            )}
                            <button type="submit" className="btn btn--primary" disabled={saving}>
                                {saving ? t("common.saving") : t("common.save")}
                            </button>
                        </div>
                    )}
                </form>
            </div>
        </section>
    );
}
