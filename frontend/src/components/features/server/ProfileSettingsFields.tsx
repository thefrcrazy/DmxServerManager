import { useState } from "react";
import { GameProfile, JsonSchemaProperty } from "@/schemas/api";
import { useLanguage } from "@/contexts/LanguageContext";
import { ProfileValue } from "@/utils/profileSettings";

type Translate = (key: string) => string;
type SectionId = "software" | "identity" | "world" | "network" | "access" | "gameplay" | "runtime" | "backups" | "advanced" | "general";

const SECTION_ORDER: readonly SectionId[] = [
    "software",
    "identity",
    "world",
    "network",
    "access",
    "gameplay",
    "runtime",
    "backups",
    "advanced",
    "general",
];

const FIELD_SECTIONS: Record<string, SectionId> = {
    loader: "software",
    version: "software",
    loader_version: "software",
    server_name: "identity",
    identity: "identity",
    world_name: "world",
    level_name: "world",
    game_name: "world",
    seed: "world",
    world_size: "world",
    port: "network",
    port_v6: "network",
    query_port: "network",
    steam_port: "network",
    steam_query_port: "network",
    rcon_port: "network",
    reliable_port: "network",
    auth_mode: "access",
    online_mode: "access",
    allow_list: "access",
    enable_lan_visibility: "access",
    default_player_permission_level: "access",
    texturepack_required: "access",
    eula_accepted: "access",
    crossplay: "access",
    public_server: "access",
    allow_op: "access",
    gamemode: "gameplay",
    difficulty: "gameplay",
    max_players: "gameplay",
    view_distance: "gameplay",
    tick_distance: "gameplay",
    player_idle_timeout: "gameplay",
    max_memory_mb: "runtime",
    automatic_backups: "backups",
    backup_frequency_minutes: "backups",
    disable_sentry: "advanced",
    accept_early_plugins: "advanced",
};

const STEAM_APP_IDS: Record<string, number> = {
    valheim: 896660,
    palworld: 2394010,
    satisfactory: 1690800,
    "seven-days-to-die": 294420,
    "project-zomboid": 380870,
    rust: 258550,
};

function translatedOr(t: Translate, key: string, fallback: string): string {
    const translated = t(key);
    return translated === key ? fallback : translated;
}

export function profileSettingTitle(t: Translate, name: string, property: JsonSchemaProperty): string {
    return translatedOr(
        t,
        `server_detail.profile_config.fields.${name}.title`,
        property.title ?? name.replaceAll("_", " "),
    );
}

function profileSettingDescription(t: Translate, name: string, property: JsonSchemaProperty): string | undefined {
    const key = `server_detail.profile_config.fields.${name}.description`;
    const translated = t(key);
    return translated === key ? property.description : translated;
}

function optionLabel(t: Translate, option: string): string {
    return translatedOr(t, `server_detail.profile_config.options.${option}`, option);
}

function profileRuntime(profile: GameProfile, t: Translate): string {
    if (profile.id === "hytale") return t("server_detail.profile_config.runtimes.java_25");
    if (profile.id === "minecraft-java" || profile.id.startsWith("minecraft-java-")) {
        return t("server_detail.profile_config.runtimes.managed_java");
    }
    if (profile.id === "minecraft-bedrock") return t("server_detail.profile_config.runtimes.official_binary");
    const steamAppId = profile.steam_profile?.app_id ?? STEAM_APP_IDS[profile.id];
    if (steamAppId) {
        return `${t("server_detail.profile_config.runtimes.steamcmd")} · AppID ${steamAppId}`;
    }
    return t("server_detail.profile_config.runtimes.native");
}

function configuredPort(profile: GameProfile, values: Record<string, ProfileValue>, index: number): string {
    const specification = profile.ports[index];
    const configured = values[specification.name];
    const value = typeof configured === "number" ? configured : specification.default;
    return `${specification.name}: ${value}/${specification.protocol.toUpperCase()}`;
}

export function ProfileConfigurationOverview({ profile, values, activeRevision }: {
    profile: GameProfile;
    values: Record<string, ProfileValue>;
    activeRevision: number;
}) {
    const { t } = useLanguage();
    const platforms = profile.platforms
        .map((platform) => translatedOr(t, `server_detail.profile_config.platforms.${platform}`, platform))
        .join(" · ");
    const ports = profile.ports.length > 0
        ? profile.ports.map((_, index) => configuredPort(profile, values, index)).join(" · ")
        : t("server_detail.profile_config.no_port");
    const compatibleUpgrade = profile.revision > activeRevision
        && Array.isArray(profile.ui_schema.compatible_from)
        && profile.ui_schema.compatible_from.includes(activeRevision);

    return (
        <div className="profile-config-overview">
            <div className="profile-config-overview__heading">
                <div>
                    <p className="profile-config-overview__eyebrow">{profile.name}</p>
                    <h2>{t("server_detail.profile_config.title")}</h2>
                </div>
                {compatibleUpgrade && (
                    <span className="badge badge--info">
                        {t("server_detail.profile_config.compatible_upgrade")}
                    </span>
                )}
            </div>
            <div className="profile-config-facts">
                <div className="profile-config-fact">
                    <span>{t("server_detail.profile_config.runtime")}</span>
                    <strong>{profileRuntime(profile, t)}</strong>
                </div>
                <div className="profile-config-fact">
                    <span>{t("server_detail.profile_config.network")}</span>
                    <strong>{ports}</strong>
                </div>
                <div className="profile-config-fact">
                    <span>{t("server_detail.profile_config.platform")}</span>
                    <strong>{platforms}</strong>
                </div>
            </div>
            <p className="profile-config-overview__hint">{t("server_detail.profile_config.apply_hint")}</p>
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

function ComplexField({ name, property, value, required, onChange }: FieldProps) {
    const { t } = useLanguage();
    const initial = JSON.stringify(value ?? (property.type === "array" ? [] : {}), null, 2);
    const [draft, setDraft] = useState(initial);
    const [invalid, setInvalid] = useState(false);
    const description = profileSettingDescription(t, name, property);

    return (
        <div className="form-group">
            <label htmlFor={`setting-${name}`}>{profileSettingTitle(t, name, property)}</label>
            <textarea
                id={`setting-${name}`}
                className="input profile-setting-textarea"
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
                {invalid
                    ? t("server_detail.profile_config.invalid_json")
                    : description ?? t("server_detail.profile_config.validated_json")}
            </p>
        </div>
    );
}

export function ProfileSettingField(props: FieldProps) {
    const { t } = useLanguage();
    const { name, property, value, required, onChange, options, optionsLoading = false } = props;
    const label = profileSettingTitle(t, name, property);
    const description = profileSettingDescription(t, name, property);
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
                    className="input profile-setting-textarea"
                    value={Array.isArray(value) ? value.map(String).join("\n") : ""}
                    onChange={(event) => onChange(event.target.value.split("\n").map((item) => item.trim()).filter(Boolean))}
                    required={required}
                    rows={4}
                />
                {description && <p className="helper-text">{description}</p>}
            </div>
        );
    }
    if (property.type === "boolean") {
        return (
            <label className="profile-setting-checkbox">
                <input type="checkbox" checked={Boolean(value ?? property.default)} onChange={(event) => onChange(event.target.checked)} required={required} />
                <span>
                    <strong>{label}</strong>
                    {description && <small>{description}</small>}
                </span>
            </label>
        );
    }
    const selectOptions = options ?? property.enum?.map(String);
    if (selectOptions) {
        return (
            <div className="form-group">
                <label htmlFor={`setting-${name}`}>{label}</label>
                <select id={`setting-${name}`} className="input" value={String(value ?? property.default ?? "")} onChange={(event) => onChange(event.target.value)} required={required} disabled={optionsLoading}>
                    {(required || selectOptions.length === 0) && <option value="">{t(optionsLoading ? "server_creation.catalog_loading" : "server_creation.catalog_select")}</option>}
                    {selectOptions.map((option) => <option key={option} value={option}>{optionLabel(t, option)}</option>)}
                </select>
                {description && <p className="helper-text">{description}</p>}
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
                value={typeof value === "object" ? "" : String(value ?? property.default ?? "")}
                required={required}
                min={property.minimum}
                max={property.maximum}
                minLength={property.minLength}
                maxLength={property.maxLength}
                autoComplete={property.secret || property.writeOnly ? "new-password" : "off"}
                onChange={(event) => onChange(numeric ? Number(event.target.value) : event.target.value)}
            />
            {description && <p className="helper-text">{description}</p>}
        </div>
    );
}

interface ProfileSettingsFieldsProps {
    profile: GameProfile;
    values: Record<string, ProfileValue>;
    onChange: (name: string, value: ProfileValue, secret: boolean) => void;
    includeSecrets?: boolean;
    options?: Record<string, readonly string[]>;
    loadingOptions?: readonly string[];
    grouped?: boolean;
}

export function ProfileSettingsFields({
    profile,
    values,
    onChange,
    includeSecrets = true,
    options = {},
    loadingOptions = [],
    grouped = false,
}: ProfileSettingsFieldsProps) {
    const { t } = useLanguage();
    const selectedMinecraftLoader = profile.id === "minecraft-java" && typeof values.loader === "string"
        ? values.loader
        : null;
    const loaderVersionRequired = selectedMinecraftLoader !== null
        && ["fabric", "forge", "neoforge", "purpur", "quilt"].includes(selectedMinecraftLoader);
    const fields = Object.entries(profile.settings_schema.properties)
        .filter(([, property]) => includeSecrets || (!property.secret && !property.writeOnly))
        .filter(([name]) => name !== "loader_version" || selectedMinecraftLoader === null || loaderVersionRequired);

    const renderField = ([name, property]: [string, JsonSchemaProperty]) => (
        <ProfileSettingField
            key={name}
            name={name}
            property={property}
            value={values[name]}
            required={profile.settings_schema.required.includes(name) || (name === "loader_version" && loaderVersionRequired)}
            options={options[name]}
            optionsLoading={loadingOptions.includes(name)}
            onChange={(value) => onChange(name, value, Boolean(property.secret || property.writeOnly))}
        />
    );

    if (!grouped) return fields.map(renderField);

    return (
        <div className="profile-settings-sections">
            {SECTION_ORDER.map((section) => {
                const sectionFields = fields.filter(([name]) => {
                    const fieldSection = profile.ports.some((port) => port.name === name)
                        ? "network"
                        : FIELD_SECTIONS[name] ?? "general";
                    return fieldSection === section;
                });
                if (sectionFields.length === 0) return null;
                return (
                    <section className="profile-settings-section" key={section}>
                        <header>
                            <h3>{t(`server_detail.profile_config.sections.${section}.title`)}</h3>
                            <p>{t(`server_detail.profile_config.sections.${section}.description`)}</p>
                        </header>
                        <div className="profile-settings-grid">
                            {sectionFields.map(renderField)}
                        </div>
                    </section>
                );
            })}
        </div>
    );
}
