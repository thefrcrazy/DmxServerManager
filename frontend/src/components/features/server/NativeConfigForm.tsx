import { AlertTriangle, Check, Pencil, Save, SlidersHorizontal, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui";
import { useLanguage } from "@/contexts/LanguageContext";
import { useToast } from "@/contexts/ToastContext";
import type { ConfigFileDocument, ConfigFileSummary } from "@/schemas/operations";
import { apiService } from "@/services";
import {
    parseNativeConfig,
    type NativeConfigField,
    type NativeConfigModel,
    type NativeConfigScalar,
} from "@/utils/nativeConfigForm";

interface NativeConfigFormProps {
    instanceId: string;
    file: ConfigFileSummary;
    canWrite: boolean;
    onChanged: () => void;
}

const MAX_VISIBLE_FIELDS = 12;

function valuesFor(model: NativeConfigModel): Record<string, NativeConfigScalar> {
    return Object.fromEntries(model.fields.map((field) => [field.id, field.value]));
}

export default function NativeConfigForm({ instanceId, file, canWrite, onChanged }: NativeConfigFormProps) {
    const { t } = useLanguage();
    const toast = useToast();
    const [document, setDocument] = useState<ConfigFileDocument | null>(null);
    const [model, setModel] = useState<NativeConfigModel | null>(null);
    const [values, setValues] = useState<Record<string, NativeConfigScalar>>({});
    const [editing, setEditing] = useState(false);
    const [loading, setLoading] = useState(true);
    const [saving, setSaving] = useState(false);
    const [error, setError] = useState("");

    const loadDocument = (nextDocument: ConfigFileDocument) => {
        const source = nextDocument.queued_content ?? nextDocument.content;
        try {
            const nextModel = parseNativeConfig(nextDocument.file.format, source);
            setDocument(nextDocument);
            setModel(nextModel);
            setValues(valuesFor(nextModel));
            setError("");
        } catch {
            setDocument(nextDocument);
            setModel(null);
            setValues({});
            setError(t("server_detail.native_config.safe_form_invalid"));
        }
    };

    useEffect(() => {
        let active = true;
        setLoading(true);
        void apiService.config.read(instanceId, file.path).then((response) => {
            if (!active) return;
            setLoading(false);
            if (!response.success) {
                setError(response.error.message);
                return;
            }
            loadDocument(response.data);
        });
        return () => { active = false; };
        // The editor deliberately keeps its baseline while open. A server-side
        // change is caught by the SHA precondition when the draft is queued.
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [file.path, instanceId]);

    const source = document ? document.queued_content ?? document.content : "";
    const serialized = useMemo(() => model ? model.serialize(values) : source, [model, source, values]);
    const dirty = Boolean(model) && serialized !== source;
    const numberFieldsValid = model?.fields.every((field) => field.kind !== "number"
        || (typeof values[field.id] === "number" && Number.isFinite(values[field.id]))) ?? true;
    const visibleFields = model?.fields.slice(0, MAX_VISIBLE_FIELDS) ?? [];
    const hiddenCount = Math.max(0, (model?.fields.length ?? 0) - visibleFields.length);

    const updateValue = (field: NativeConfigField, value: NativeConfigScalar) => {
        setValues((current) => ({ ...current, [field.id]: value }));
    };

    const cancelEditing = () => {
        if (model) setValues(valuesFor(model));
        setEditing(false);
        setError("");
    };

    const queue = async () => {
        if (!document || !model || !dirty || !numberFieldsValid) return;
        setSaving(true);
        const response = await apiService.config.queue(instanceId, file.path, serialized, document.file.sha256);
        setSaving(false);
        if (!response.success) {
            const message = response.error.status === 409
                ? t("server_detail.native_config.source_changed")
                : response.error.message;
            setError(message);
            toast.error(message);
            return;
        }
        loadDocument(response.data);
        setEditing(false);
        toast.success(t("server_detail.native_config.queued"));
        onChanged();
    };

    if (loading) return <div className="native-safe-form__loading"><span className="spinner spinner--sm" />{t("common.loading")}</div>;

    return (
        <section className="native-safe-form" aria-label={t("server_detail.native_config.safe_form_title")}>
            <header className="native-safe-form__header">
                <div className="native-safe-form__heading">
                    <span className="native-safe-form__icon"><SlidersHorizontal size={17} aria-hidden="true" /></span>
                    <span><strong>{t("server_detail.native_config.safe_form_title")}</strong><small>{t("server_detail.native_config.safe_form_hint")}</small></span>
                </div>
                {canWrite && model && model.fields.length > 0 && !editing && (
                    <Button type="button" size="sm" variant="secondary" icon={<Pencil size={15} />} onClick={() => setEditing(true)}>
                        {t("common.edit")}
                    </Button>
                )}
            </header>

            {error && <p className="native-safe-form__error" role="alert"><AlertTriangle size={15} />{error}</p>}
            {!error && model && model.fields.length === 0 && <p className="native-safe-form__empty">{t("server_detail.native_config.safe_form_empty")}</p>}

            {visibleFields.length > 0 && (
                <div className={`native-safe-form__grid ${editing ? "is-editing" : ""}`}>
                    {visibleFields.map((field) => {
                        const inputId = `${file.path}-${field.id}`.replaceAll(/[^a-zA-Z0-9_-]/g, "-");
                        const value = values[field.id] ?? field.value;
                        if (!editing) {
                            const displayValue = field.kind === "secret"
                                ? t(field.configured ? "server_detail.native_config.configured_value" : "server_detail.native_config.not_configured_value")
                                : field.kind === "boolean"
                                    ? t(value ? "server_detail.native_config.enabled_value" : "server_detail.native_config.disabled_value")
                                    : value === ""
                                        ? "—"
                                        : String(value);
                            return <div className="native-safe-form__value" key={field.id}><span>{field.label}{field.section && <small>{field.section}</small>}</span><strong className={field.kind === "secret" ? "is-secret" : ""}>{field.kind === "secret" && field.configured && <Check size={13} />}{displayValue}</strong></div>;
                        }
                        if (field.kind === "boolean") {
                            return <label className="native-safe-form__boolean" htmlFor={inputId} key={field.id}><input id={inputId} type="checkbox" checked={Boolean(value)} onChange={(event) => updateValue(field, event.target.checked)} /><span><strong>{field.label}</strong>{field.section && <small>{field.section}</small>}</span></label>;
                        }
                        return <label className="native-safe-form__field" htmlFor={inputId} key={field.id}><span>{field.label}{field.section && <small>{field.section}</small>}</span><input id={inputId} className="input" type={field.kind === "secret" ? "password" : field.kind === "number" ? "number" : "text"} value={String(value)} placeholder={field.kind === "secret" && field.configured ? t("server_detail.native_config.keep_configured_value") : undefined} autoComplete={field.kind === "secret" ? "new-password" : "off"} onChange={(event) => updateValue(field, field.kind === "number" ? event.target.value === "" ? "" : Number(event.target.value) : event.target.value)} /></label>;
                    })}
                </div>
            )}

            {hiddenCount > 0 && <p className="native-safe-form__more">{t("server_detail.native_config.safe_form_more").replace("{{count}}", String(hiddenCount))}</p>}
            {!numberFieldsValid && <p className="native-safe-form__error" role="alert"><AlertTriangle size={15} />{t("server_detail.native_config.invalid_number")}</p>}

            {editing && <footer className="native-safe-form__actions">
                <Button type="button" size="sm" variant="ghost" icon={<X size={15} />} onClick={cancelEditing}>{t("common.cancel")}</Button>
                <Button type="button" size="sm" icon={<Save size={15} />} disabled={!dirty || !numberFieldsValid} isLoading={saving} onClick={() => void queue()}>{t("server_detail.native_config.queue_form")}</Button>
            </footer>}
        </section>
    );
}
