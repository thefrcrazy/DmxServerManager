import { useState } from "react";
import { GameProfile, JsonSchemaProperty } from "@/schemas/api";
import { useLanguage } from "@/contexts/LanguageContext";
import { ProfileValue } from "@/utils/profileSettings";

function ComplexField({ name, property, value, required, onChange }: FieldProps) {
    const initial = JSON.stringify(value ?? (property.type === "array" ? [] : {}), null, 2);
    const [draft, setDraft] = useState(initial);
    const [invalid, setInvalid] = useState(false);

    return (
        <div className="form-group">
            <label htmlFor={`setting-${name}`}>{property.title ?? name.replaceAll("_", " ")}</label>
            <textarea
                id={`setting-${name}`}
                className="input"
                value={draft}
                required={required}
                rows={6}
                spellCheck={false}
                onChange={(event) => {
                    const next = event.target.value;
                    setDraft(next);
                    try {
                        const parsed = JSON.parse(next) as unknown;
                        const validShape = property.type === "array"
                            ? Array.isArray(parsed)
                            : typeof parsed === "object" && parsed !== null && !Array.isArray(parsed);
                        setInvalid(!validShape);
                        if (validShape) onChange(parsed as ProfileValue);
                    } catch {
                        setInvalid(true);
                    }
                }}
                aria-invalid={invalid}
            />
            <p className={`helper-text ${invalid ? "text-danger" : ""}`}>
                {invalid ? "JSON invalide" : property.description ?? "Valeur JSON validée côté serveur."}
            </p>
        </div>
    );
}

interface FieldProps {
    name: string;
    property: JsonSchemaProperty;
    value: ProfileValue | undefined;
    required: boolean;
    onChange: (value: ProfileValue) => void;
    options?: readonly string[];
    optionsLoading?: boolean;
}

export function ProfileSettingField(props: FieldProps) {
    const { t } = useLanguage();
    const { name, property, value, required, onChange, options, optionsLoading = false } = props;
    const label = property.title ?? name.replaceAll("_", " ");
    const itemType = typeof property.items === "object" && property.items !== null && "type" in property.items
        ? (property.items as { type?: unknown }).type
        : undefined;

    if (property.type === "object" || (property.type === "array" && itemType === "object")) {
        return <ComplexField {...props} />;
    }
    if (property.type === "array") {
        return (
            <div className="form-group">
                <label htmlFor={`setting-${name}`}>{label}</label>
                <textarea
                    id={`setting-${name}`}
                    className="input"
                    value={Array.isArray(value) ? value.map(String).join("\n") : ""}
                    onChange={(event) => onChange(event.target.value.split("\n").map((item) => item.trim()).filter(Boolean))}
                    required={required}
                    rows={4}
                />
            </div>
        );
    }
    if (property.type === "boolean") {
        return (
            <label className="form-checkbox">
                <input type="checkbox" checked={Boolean(value)} onChange={(event) => onChange(event.target.checked)} required={required} />
                <span>{label}</span>
            </label>
        );
    }
    const selectOptions = options ?? property.enum?.map(String);
    if (selectOptions) {
        return (
            <div className="form-group">
                <label htmlFor={`setting-${name}`}>{label}</label>
                <select id={`setting-${name}`} className="input" value={String(value ?? "")} onChange={(event) => onChange(event.target.value)} required={required} disabled={optionsLoading}>
                    {(required || selectOptions.length === 0) && <option value="">{t(optionsLoading ? "server_creation.catalog_loading" : "server_creation.catalog_select")}</option>}
                    {selectOptions.map((option) => <option key={option} value={option}>{option}</option>)}
                </select>
                {property.description && <p className="helper-text">{property.description}</p>}
            </div>
        );
    }
    const numeric = property.type === "integer" || property.type === "number";
    return (
        <div className="form-group">
            <label htmlFor={`setting-${name}`}>{label}</label>
            <input
                id={`setting-${name}`}
                className="input"
                type={numeric ? "number" : property.format === "password" || property.secret || property.writeOnly ? "password" : "text"}
                value={typeof value === "object" ? "" : String(value ?? "")}
                required={required}
                min={property.minimum}
                max={property.maximum}
                minLength={property.minLength}
                maxLength={property.maxLength}
                autoComplete={property.secret || property.writeOnly ? "new-password" : "off"}
                onChange={(event) => onChange(numeric ? Number(event.target.value) : event.target.value)}
            />
            {property.description && <p className="helper-text">{property.description}</p>}
        </div>
    );
}

export function ProfileSettingsFields({ profile, values, onChange, includeSecrets = true, options = {}, loadingOptions = [] }: {
    profile: GameProfile;
    values: Record<string, ProfileValue>;
    onChange: (name: string, value: ProfileValue, secret: boolean) => void;
    includeSecrets?: boolean;
    options?: Record<string, readonly string[]>;
    loadingOptions?: readonly string[];
}) {
    return Object.entries(profile.settings_schema.properties)
        .filter(([, property]) => includeSecrets || (!property.secret && !property.writeOnly))
        .map(([name, property]) => (
            <ProfileSettingField
                key={name}
                name={name}
                property={property}
                value={values[name]}
                required={profile.settings_schema.required.includes(name)}
                options={options[name]}
                optionsLoading={loadingOptions.includes(name)}
                onChange={(value) => onChange(name, value, Boolean(property.secret || property.writeOnly))}
            />
        ));
}
