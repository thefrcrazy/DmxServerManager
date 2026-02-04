export const enhanceLogContent = (content: string, gameType: string = "hytale"): string => {
    // Common enhancements (ANSI stripping, timestamps) could go here or be game-type specific

    // Hytale specific logic
    if (gameType === "hytale") {
        return content
            // Fix excessive whitespace (2 or more spaces -> 1 space), preserving indentation if needed but usually we want to collapse
            .replace(/\s{2,}/g, " ")
            // Strip potentially broken/visible ANSI bracket sequences like [0m] or [0;39m] appearing as text
            .replace(/\[\d+(;\d+)*m\]/g, "")
            // Time stamps: [HH:mm:ss] or [YYYY/MM/DD HH:mm:ss] -> Gray
            .replace(/(\[\d{4}\/\d{2}\/\d{2}\s\d{2}:\d{2}:\d{2}\])|(\[\d{2}:\d{2}:\d{2}\])/g, "\u001b[90m$&\u001b[0m")
            // Log Levels
            .replace(/(\bINFO\b)|(\[INFO\])/g, "\u001b[36m$&\u001b[0m") // Cyan for INFO
            .replace(/(\bWARN\b)|(\bWARNING\b)|(\[WARN\])/g, "\u001b[33m$&\u001b[0m") // Yellow for WARN
            .replace(/(\bERROR\b)|(\bERR\b)|(\[ERROR\])|(\[ERR\])/g, "\u001b[31m$&\u001b[0m") // Red for ERROR
            .replace(/(\bFATAL\b)|(\[FATAL\])|(\bSEVERE\b)|(\[SEVERE\])/g, "\u001b[31;1m$&\u001b[0m") // Bold Red for FATAL/SEVERE
            // Components: [Component] -> Magenta/Purple - refined to avoid matching [0m] if missed or numbers, and support |
            .replace(/(\[[a-zA-Z][a-zA-Z0-9_.\-|:]*\])(?=\s)/g, (match) => {
                // Ensure it starts with a letter to avoid matching [0m] or [123] unless it's a component
                if (match.includes("INFO") || match.includes("WARN") || match.includes("ERR") || match.includes("FATAL") || match.includes("SEVERE")) return match;
                return `\u001b[35m${match}\u001b[0m`;
            });
    }

    // Default Fallback (just strip broken ansi brackets if any, or return as is)
    // For now we can apply at least the basic ANSI fix for everyone
    return content.replace(/\[\d+(;\d+)*m\]/g, "");
};
