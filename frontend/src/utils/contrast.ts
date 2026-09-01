// Pendant client de `backend/src/core/color.rs`. Les deux implémentations
// doivent rester d'accord : le serveur refuse une couleur non conforme, le
// client l'annonce avant l'envoi plutôt que de laisser l'utilisateur découvrir
// l'erreur après coup.

const HEX_COLOR = /^#[0-9a-f]{6}$/i;

/** Seuil WCAG 1.4.11 pour un composant d'interface : bordure, icône, focus. */
export const UI_COMPONENT_CONTRAST = 3;

/** Seuil WCAG 1.4.3 pour du texte de taille normale. */
export const TEXT_CONTRAST = 4.5;

export function isHexColor(value: string | null | undefined): value is string {
    return typeof value === "string" && HEX_COLOR.test(value);
}

export function parseColor(value: string): [number, number, number] | null {
    if (!isHexColor(value)) return null;
    return [1, 3, 5].map((offset) => Number.parseInt(value.slice(offset, offset + 2), 16)) as [number, number, number];
}

export function relativeLuminance(color: [number, number, number]): number {
    const weights = [0.2126, 0.7152, 0.0722];
    return color
        .map((channel) => channel / 255)
        .map((channel) => channel <= 0.03928 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4)
        .reduce((total, channel, index) => total + channel * (weights[index] ?? 0), 0);
}

export function contrastRatio(left: string, right: string): number | null {
    const first = parseColor(left);
    const second = parseColor(right);
    if (!first || !second) return null;
    const lighter = Math.max(relativeLuminance(first), relativeLuminance(second));
    const darker = Math.min(relativeLuminance(first), relativeLuminance(second));
    return (lighter + 0.05) / (darker + 0.05);
}

/** Noir ou blanc, celui qui contraste le mieux avec la couleur donnée. */
export function readableTextOn(color: string): "#000000" | "#ffffff" {
    const parsed = parseColor(color);
    if (!parsed) return "#ffffff";
    const luminance = relativeLuminance(parsed);
    return (luminance + 0.05) / 0.05 >= 1.05 / (luminance + 0.05) ? "#000000" : "#ffffff";
}
