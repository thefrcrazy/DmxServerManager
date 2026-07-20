import { AlertTriangle, ListChecks, Save } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui";
import { useLanguage } from "@/contexts/LanguageContext";
import { useToast } from "@/contexts/ToastContext";
import type { ConfigFileDocument, ConfigFileSummary } from "@/schemas/operations";
import { apiService } from "@/services";

interface NativeAccessListFormProps {
    instanceId: string;
    file: ConfigFileSummary;
    canWrite: boolean;
    onChanged: () => void;
}

const MAX_ENTRIES = 5_000;
const MAX_LINE_LENGTH = 128;

function normalizeList(value: string): string {
    return value.replace(/\r\n?/g, "\n").replace(/\n+$/g, "");
}

function listIsValid(value: string): boolean {
    const lines = normalizeList(value).split("\n");
    return lines.length <= MAX_ENTRIES && lines.every((line) => (
        line.length <= MAX_LINE_LENGTH
        && ![...line].some((character) => {
            const code = character.codePointAt(0) ?? 0;
            return code < 32 && character !== "\t";
        })
    ));
}

export default function NativeAccessListForm({
    instanceId,
    file,
    canWrite,
    onChanged,
}: NativeAccessListFormProps) {
    const { t } = useLanguage();
    const toast = useToast();
    const [fileDocument, setFileDocument] = useState<ConfigFileDocument | null>(null);
    const [draft, setDraft] = useState("");
    const [loading, setLoading] = useState(true);
    const [saving, setSaving] = useState(false);
    const [error, setError] = useState("");

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
            setFileDocument(response.data);
            setDraft(response.data.queued_content ?? response.data.content);
            setError("");
        });
        return () => { active = false; };
    }, [file.path, instanceId]);

    const normalized = useMemo(() => normalizeList(draft), [draft]);
    const valid = useMemo(() => listIsValid(draft), [draft]);
    const baseline = fileDocument ? normalizeList(fileDocument.queued_content ?? fileDocument.content) : "";
    const dirty = normalized !== baseline;
    const filename = file.path.split("/").at(-1) ?? file.path;
    const inputId = `native-list-${file.path.replaceAll(/[^a-zA-Z0-9]/g, "-")}`;

    const queue = async () => {
        if (!fileDocument || !valid || !dirty) return;
        setSaving(true);
        const response = await apiService.config.queue(
            instanceId,
            file.path,
            normalized,
            fileDocument.file.sha256,
        );
        setSaving(false);
        if (!response.success) {
            toast.error(response.error.message);
            return;
        }
        setFileDocument(response.data);
        setDraft(response.data.queued_content ?? response.data.content);
        toast.success(t("server_detail.native_config.queued"));
        onChanged();
    };

    if (loading) return <div className="native-list-form__loading"><span className="spinner spinner--sm" />{t("common.loading")}</div>;
    if (error) return <div className="native-list-form__error" role="alert">{error}</div>;

    return (
        <div className="native-list-form">
            <div className="native-list-form__heading">
                <div><ListChecks size={17} aria-hidden="true" /><strong>{t("server_detail.native_config.safe_list")}</strong></div>
                <small>{t("server_detail.native_config.safe_list_hint")}</small>
            </div>
            <label htmlFor={inputId}>{t("server_detail.native_config.list_entries")} — {filename}</label>
            <textarea
                id={inputId}
                className="input native-list-form__textarea"
                rows={Math.min(10, Math.max(4, normalized.split("\n").length + 1))}
                value={draft}
                readOnly={!canWrite}
                spellCheck={false}
                aria-invalid={!valid}
                onChange={(event) => setDraft(event.target.value)}
            />
            {!valid && <p className="native-list-form__validation" role="alert"><AlertTriangle size={15} />{t("server_detail.native_config.invalid_list")}</p>}
            {canWrite && <div className="native-list-form__actions">
                <Button type="button" size="sm" icon={<Save size={15} />} disabled={!dirty || !valid} isLoading={saving} onClick={() => void queue()}>{t("server_detail.native_config.queue_list")}</Button>
            </div>}
        </div>
    );
}
