import { useState } from "react";
import { Check, Clipboard, ExternalLink, KeyRound } from "lucide-react";
import { Button } from "@/components/ui";
import { useLanguage } from "@/contexts/LanguageContext";
import { useFocusTrap } from "@/hooks";
import { HytaleDeviceAuthorization } from "@/schemas/operations";

/**
 * Demande d'autorisation par appareil, au centre de l'écran.
 *
 * Le bloc ne portait aucune règle de style — la classe n'existait dans aucune
 * feuille — et se rendait donc en pile brute en tête de page : sans marge, sans
 * séparation, le libellé et le code se touchaient (« Code utilisateuraEg77ciT »)
 * et le lien d'ouverture se perdait au bout d'une ligne. C'est pourtant l'écran
 * qui bloque l'installation : il doit être la chose la plus visible de la page.
 */
export default function HytaleDeviceAuthorizationNotice({
    authorization,
}: {
    authorization: HytaleDeviceAuthorization;
}) {
    const { t } = useLanguage();
    const [copied, setCopied] = useState(false);
    // Réductible plutôt que fermable : l'installation attend toujours, donc
    // faire disparaître la demande laisserait un job bloqué sans explication.
    const [collapsed, setCollapsed] = useState(false);
    const { containerRef, onKeyDown } = useFocusTrap<HTMLDivElement>({
        active: !collapsed,
        onEscape: () => setCollapsed(true),
    });
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

    const openButton = (
        <a
            className="btn btn--primary"
            href={authorization.interaction.verification_uri}
            target="_blank"
            rel="noopener noreferrer"
        >
            <ExternalLink size={16} aria-hidden="true" />
            {t("hytale_device.open")}
        </a>
    );

    if (collapsed) {
        return (
            <section className="hytale-device-auth" role="status" aria-live="polite">
                <span className="hytale-device-auth__icon">
                    <KeyRound size={18} aria-hidden="true" />
                </span>
                <p>{t("hytale_device.title")}</p>
                {openButton}
                <Button variant="ghost" size="sm" onClick={() => setCollapsed(false)}>
                    {t("hytale_device.reopen")}
                </Button>
            </section>
        );
    }

    return (
        <div className="dialog-overlay">
            <div
                ref={containerRef}
                className="dialog hytale-device-modal"
                role="dialog"
                aria-modal="true"
                aria-labelledby="hytale-device-auth-title"
                tabIndex={-1}
                onKeyDown={onKeyDown}
            >
                <span className="hytale-device-modal__icon">
                    <KeyRound size={26} aria-hidden="true" />
                </span>
                <h2 id="hytale-device-auth-title">{t("hytale_device.title")}</h2>
                <p>{t("hytale_device.description")}</p>

                {/* Le code est la donnée à recopier : il mérite d'être lisible de
                    loin, chiffre par chiffre, plutôt que collé à son libellé. */}
                {code && (
                    <div className="hytale-device-modal__code">
                        <span>{t("hytale_device.user_code")}</span>
                        <code>{code}</code>
                        <Button
                            type="button"
                            size="sm"
                            variant="secondary"
                            onClick={() => void copy()}
                            icon={copied ? <Check size={15} /> : <Clipboard size={15} />}
                        >
                            {t(copied ? "hytale_device.copied" : "hytale_device.copy")}
                        </Button>
                    </div>
                )}

                <div className="hytale-device-modal__actions">
                    {openButton}
                    <Button variant="ghost" size="sm" onClick={() => setCollapsed(true)}>
                        {t("hytale_device.collapse")}
                    </Button>
                </div>

                <p className="helper-text">{t("hytale_device.content_blocker_hint")}</p>
            </div>
        </div>
    );
}
