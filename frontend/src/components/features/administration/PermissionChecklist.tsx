import { ShieldAlert } from "lucide-react";
import { useLanguage } from "@/contexts/LanguageContext";
import { PermissionDescription, PermissionId } from "@/schemas/api";
import { permissionLabel } from "@/utils/roles";

interface PermissionChecklistProps {
    catalog: PermissionDescription[];
    selected: PermissionId[];
    onChange: (permissions: PermissionId[]) => void;
    legend: string;
    disabled?: boolean;
}

export default function PermissionChecklist({
    catalog,
    selected,
    onChange,
    legend,
    disabled = false,
}: PermissionChecklistProps) {
    const { t } = useLanguage();
    const selectedSet = new Set(selected);

    const toggle = (permission: PermissionId, checked: boolean) => {
        const next = new Set(selectedSet);
        if (checked) next.add(permission);
        else next.delete(permission);
        onChange([...next].sort());
    };

    return (
        <fieldset className="permission-fieldset" disabled={disabled}>
            <legend>{legend}</legend>
            <div className="permission-grid">
                {catalog.map((permission) => (
                    <label
                        className={`permission-option ${permission.high_risk ? "permission-option--high-risk" : ""}`}
                        key={permission.id}
                    >
                        <input
                            type="checkbox"
                            checked={selectedSet.has(permission.id)}
                            onChange={(event) => toggle(permission.id, event.target.checked)}
                        />
                        <span className="permission-option__content">
                            <span className="permission-option__label">
                                {permissionLabel(permission.id, t)}
                                {permission.high_risk && (
                                    <span className="permission-option__risk">
                                        <ShieldAlert size={14} aria-hidden="true" />
                                        {t("administration.roles.high_risk")}
                                    </span>
                                )}
                            </span>
                            <code>{permission.id}</code>
                        </span>
                    </label>
                ))}
            </div>
        </fieldset>
    );
}
