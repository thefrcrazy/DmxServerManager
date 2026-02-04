export const formatBytes = (bytes: number): string => {
    if (bytes === 0) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB", "TB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + " " + sizes[i];
};

export const formatGB = (bytes: number): string => {
    if (bytes === 0) return "0.0 GB";
    const gb = bytes / (1024 * 1024 * 1024);
    return gb.toFixed(1) + " GB";
};

export const formatDate = (dateStr: string | null, locale = "fr-FR"): string => {
    if (!dateStr) return "—";
    return new Date(dateStr).toLocaleString(locale, {
        day: "2-digit",
        month: "2-digit",
        year: "numeric",
        hour: "2-digit",
        minute: "2-digit"
    });
};

export const formatDateShort = (dateStr: string | null, locale = "fr-FR"): string => {
    if (!dateStr) return "—";
    return new Date(dateStr).toLocaleDateString(locale, {
        day: "2-digit",
        month: "short",
        year: "numeric"
    });
};
