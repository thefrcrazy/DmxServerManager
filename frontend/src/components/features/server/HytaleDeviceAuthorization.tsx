import { useState } from "react";
import { Check, Clipboard, ExternalLink, KeyRound } from "lucide-react";
import { Button } from "@/components/ui";
import { useLanguage } from "@/contexts/LanguageContext";
import { HytaleDeviceAuthorization } from "@/schemas/operations";

export default function HytaleDeviceAuthorizationNotice({ authorization }: { authorization: HytaleDeviceAuthorization }) {
    const { t } = useLanguage();
    const [copied, setCopied] = useState(false);
    const code = authorization.interaction.user_code;

    const copy = async () => {
        if (!code || !navigator.clipboard) return;
        try {
            await navigator.clipboard.writeText(code);
            setCopied(true);
            window.setTimeout(() => setCopied(false), 2_000);
        } catch {
            setCopied(false);
        }
    };

    return (
        <section className="hytale-device-auth" role="status" aria-live="polite" aria-labelledby="hytale-device-auth-title">
            <span className="hytale-device-auth__icon"><KeyRound size={22} aria-hidden="true" /></span>
            <div className="hytale-device-auth__content">
                <h2 id="hytale-device-auth-title">{t("hytale_device.title")}</h2>
                <p>{t("hytale_device.description")}</p>
                {code && <div className="hytale-device-auth__code"><span>{t("hytale_device.user_code")}</span><code>{code}</code><Button type="button" size="sm" variant="secondary" onClick={() => void copy()} icon={copied ? <Check size={15} /> : <Clipboard size={15} />}>{t(copied ? "hytale_device.copied" : "hytale_device.copy")}</Button></div>}
            </div>
            <a className="btn btn--primary" href={authorization.interaction.verification_uri} target="_blank" rel="noopener noreferrer">
                <ExternalLink size={16} aria-hidden="true" />{t("hytale_device.open")}
            </a>
        </section>
    );
}
