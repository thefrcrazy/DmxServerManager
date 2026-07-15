export function queryString(values: Record<string, string | number | boolean | null | undefined>): string {
    const query = new URLSearchParams();
    for (const [key, value] of Object.entries(values)) {
        if (value !== undefined && value !== null && value !== "") query.set(key, String(value));
    }
    const encoded = query.toString();
    return encoded ? `?${encoded}` : "";
}

export function safeDownloadName(value: string, fallback: string): string {
    const cleaned = value.replace(/[\u0000-\u001f\u007f/\\:]/g, "-").trim();
    return cleaned.slice(0, 120) || fallback;
}
