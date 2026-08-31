import { Check, TriangleAlert } from "lucide-react";
import { PRESET_COLORS } from "@/constants/theme";
import { useLanguage } from "@/contexts/LanguageContext";
import { UI_COMPONENT_CONTRAST, contrastRatio } from "@/utils/contrast";

interface ColorPickerProps {
    value: string;
    onChange: (color: string) => void;
    showCustomInput?: boolean;
    /** Fond sur lequel l'accent sera rendu, pour le calcul de contraste. */
    background?: string;
}

export default function ColorPicker({
    value,
    onChange,
    showCustomInput = true,
    background = "#000000",
}: ColorPickerProps) {
    const { t } = useLanguage();
    // L'accent pilote bordures, soulignements d'onglet, icônes et anneaux de
    // focus : sous 3:1 il devient indiscernable du fond. Le serveur applique la
    // même règle ; l'avertissement évite de découvrir le refus après coup.
    const ratio = contrastRatio(value, background);
    const insufficient = ratio !== null && ratio < UI_COMPONENT_CONTRAST;

    return (
        <div className="color-picker">
            <div className="color-picker__swatches">
                {PRESET_COLORS.map((color) => (
                    <button
                        key={color}
                        type="button"
                        aria-label={`${t("user_settings.color")} ${color}`}
                        aria-pressed={value.toLowerCase() === color.toLowerCase()}
                        onClick={() => onChange(color)}
                        className={`color-picker__swatch ${value.toLowerCase() === color.toLowerCase() ? "color-picker__swatch--active" : ""}`}
                        style={{
                            background: color,
                            boxShadow: value.toLowerCase() === color.toLowerCase()
                                ? `0 0 15px ${color}66`
                                : "none"
                        }}
                    >
                        {value.toLowerCase() === color.toLowerCase() && (
                            <Check size={20} color="white" strokeWidth={3} />
                        )}
                    </button>
                ))}

                {showCustomInput && (
                    <div className="color-picker__custom">
                        <input
                            type="color"
                            value={value}
                            onChange={(e) => onChange(e.target.value)}
                            title={t("user_settings.custom_color_title")}
                            aria-label={t("user_settings.custom_color_title")}
                        />
                    </div>
                )}
            </div>

            {insufficient && (
                <p className="color-picker__warning" role="status">
                    <TriangleAlert size={16} aria-hidden="true" />
                    {t("user_settings.accent_contrast_warning")}
                </p>
            )}
        </div>
    );
}
