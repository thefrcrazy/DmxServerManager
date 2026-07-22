import type { ConfigFileSummary } from "@/schemas/operations";

export type NativeConfigScalar = string | number | boolean;

export interface NativeConfigField {
    id: string;
    label: string;
    section: string | null;
    kind: "string" | "number" | "boolean" | "secret";
    value: NativeConfigScalar;
    configured: boolean;
}

export interface NativeConfigModel {
    fields: NativeConfigField[];
    serialize: (values: Readonly<Record<string, NativeConfigScalar>>) => string;
}

interface ReplacementField {
    field: NativeConfigField;
    start: number;
    end: number;
    original: string;
    quote: "\"" | "'" | null;
}

const MAX_SAFE_FIELDS = 256;
const SECRET_PATTERN = /(?:password|passwd|secret|token|private.?key|api.?key|rcon.?password)/i;

export function supportsNativeConfigForm(format: ConfigFileSummary["format"]): boolean {
    return ["json", "properties", "ini", "toml", "xml"].includes(format);
}

function fieldPriority(label: string): number {
    const normalized = label.toLowerCase().replaceAll(/[^a-z0-9]/g, "");
    const priorities = [
        /servername|serverhostname|hostname|sessionname/,
        /motd|description|message/,
        /maxplayers|playerlimit/,
        /gamemode|difficulty/,
        /port/,
        /password|secret|token/,
        /world|map|level/,
        /public|visibility|whitelist|allowlist/,
    ];
    const index = priorities.findIndex((pattern) => pattern.test(normalized));
    return index < 0 ? priorities.length : index;
}

function sortFields(fields: NativeConfigField[]): NativeConfigField[] {
    return fields
        .map((field, order) => ({ field, order }))
        .sort((left, right) => fieldPriority(left.field.label) - fieldPriority(right.field.label) || left.order - right.order)
        .map(({ field }) => field);
}

function isSecret(label: string): boolean {
    return SECRET_PATTERN.test(label);
}

function decodeQuoted(raw: string): { value: string; quote: "\"" | "'" | null } {
    const trimmed = raw.trim();
    if (trimmed.length >= 2 && (trimmed[0] === "\"" || trimmed[0] === "'") && trimmed.at(-1) === trimmed[0]) {
        const quote = trimmed[0] as "\"" | "'";
        const inner = trimmed.slice(1, -1);
        return {
            value: quote === "\""
                ? inner.replaceAll(/\\([\\"])/g, "$1")
                : inner.replaceAll("''", "'"),
            quote,
        };
    }
    return { value: trimmed, quote: null };
}

function scalarField(id: string, label: string, section: string | null, raw: string): NativeConfigField {
    const { value } = decodeQuoted(raw);
    if (isSecret(label)) {
        return { id, label, section, kind: "secret", value: "", configured: value.length > 0 };
    }
    if (/^(?:true|false)$/i.test(value)) {
        return { id, label, section, kind: "boolean", value: value.toLowerCase() === "true", configured: true };
    }
    if (/^-?(?:\d+\.?\d*|\.\d+)$/.test(value) && Number.isFinite(Number(value))) {
        return { id, label, section, kind: "number", value: Number(value), configured: true };
    }
    return { id, label, section, kind: "string", value, configured: value.length > 0 };
}

function encodeScalar(value: NativeConfigScalar, original: string, quote: "\"" | "'" | null, secret: boolean): string {
    if (secret && value === "") return original;
    if (typeof value === "boolean") {
        if (original === original.toUpperCase()) return value ? "TRUE" : "FALSE";
        if (/^[A-Z]/.test(original)) return value ? "True" : "False";
        return value ? "true" : "false";
    }
    if (typeof value === "number") return Number.isFinite(value) ? String(value) : original;
    if (quote === "\"") return `"${value.replaceAll("\\", "\\\\").replaceAll("\"", "\\\"")}"`;
    if (quote === "'") return `'${value.replaceAll("'", "''")}'`;
    return value;
}

function splitInlineComment(value: string): number {
    let quote: "\"" | "'" | null = null;
    let escaped = false;
    for (let index = 0; index < value.length; index += 1) {
        const character = value[index]!;
        if (quote === "\"" && character === "\\" && !escaped) {
            escaped = true;
            continue;
        }
        if ((character === "\"" || character === "'") && !escaped) {
            quote = quote === character ? null : quote ?? character;
        }
        if (!quote && (character === "#" || character === ";") && (index === 0 || /\s/.test(value[index - 1]!))) {
            return index;
        }
        escaped = false;
    }
    return value.length;
}

function matchingClosingParenthesis(value: string, start: number): number {
    let depth = 0;
    let quote: "\"" | "'" | null = null;
    let escaped = false;
    for (let index = start; index < value.length; index += 1) {
        const character = value[index]!;
        if (quote === "\"" && character === "\\" && !escaped) {
            escaped = true;
            continue;
        }
        if ((character === "\"" || character === "'") && !escaped) quote = quote === character ? null : quote ?? character;
        if (!quote && character === "(") depth += 1;
        if (!quote && character === ")") {
            depth -= 1;
            if (depth === 0) return index;
        }
        escaped = false;
    }
    return -1;
}

function topLevelSegments(value: string): Array<{ start: number; end: number }> {
    const segments: Array<{ start: number; end: number }> = [];
    let start = 0;
    let depth = 0;
    let quote: "\"" | "'" | null = null;
    let escaped = false;
    for (let index = 0; index < value.length; index += 1) {
        const character = value[index]!;
        if (quote === "\"" && character === "\\" && !escaped) {
            escaped = true;
            continue;
        }
        if ((character === "\"" || character === "'") && !escaped) quote = quote === character ? null : quote ?? character;
        if (!quote && character === "(") depth += 1;
        if (!quote && character === ")") depth = Math.max(0, depth - 1);
        if (!quote && depth === 0 && character === ",") {
            segments.push({ start, end: index });
            start = index + 1;
        }
        escaped = false;
    }
    segments.push({ start, end: value.length });
    return segments;
}

function topLevelEquals(value: string): number {
    let depth = 0;
    let quote: "\"" | "'" | null = null;
    for (let index = 0; index < value.length; index += 1) {
        const character = value[index]!;
        if (character === "\"" || character === "'") quote = quote === character ? null : quote ?? character;
        if (!quote && character === "(") depth += 1;
        if (!quote && character === ")") depth = Math.max(0, depth - 1);
        if (!quote && depth === 0 && character === "=") return index;
    }
    return -1;
}

function parseLineModel(source: string): NativeConfigModel {
    const replacements: ReplacementField[] = [];
    let section: string | null = null;
    let sourceOffset = 0;
    let order = 0;
    const linePattern = /[^\r\n]*(?:\r\n|\n|\r|$)/g;

    for (const match of source.matchAll(linePattern)) {
        const fullLine = match[0];
        if (fullLine === "") break;
        const line = fullLine.replace(/[\r\n]+$/, "");
        const sectionMatch = line.match(/^\s*\[([^\]]+)]/);
        if (sectionMatch) {
            section = sectionMatch[1]!.trim();
            sourceOffset += fullLine.length;
            continue;
        }
        if (!/^\s*[#;]/.test(line)) {
            const equalsIndex = line.indexOf("=");
            const tupleStart = equalsIndex >= 0 ? line.indexOf("(", equalsIndex + 1) : -1;
            const tupleEnd = tupleStart >= 0 ? matchingClosingParenthesis(line, tupleStart) : -1;
            if (equalsIndex >= 0 && tupleStart >= 0 && tupleEnd > tupleStart) {
                const parentLabel = line.slice(0, equalsIndex).trim();
                const tuple = line.slice(tupleStart + 1, tupleEnd);
                for (const segment of topLevelSegments(tuple)) {
                    if (replacements.length >= MAX_SAFE_FIELDS) break;
                    const fragment = tuple.slice(segment.start, segment.end);
                    const nestedEquals = topLevelEquals(fragment);
                    if (nestedEquals < 0) continue;
                    const label = fragment.slice(0, nestedEquals).trim();
                    const rawTail = fragment.slice(nestedEquals + 1);
                    const leading = rawTail.length - rawTail.trimStart().length;
                    const trailing = rawTail.length - rawTail.trimEnd().length;
                    const original = rawTail.slice(leading, rawTail.length - trailing);
                    const start = sourceOffset + tupleStart + 1 + segment.start + nestedEquals + 1 + leading;
                    const id = `native-field-${order}`;
                    const field = scalarField(id, label, section ?? parentLabel, original);
                    replacements.push({ field, start, end: start + original.length, original, quote: decodeQuoted(original).quote });
                    order += 1;
                }
                sourceOffset += fullLine.length;
                continue;
            }

            const assignment = line.match(/^(\s*)([^=]+?)(\s*=\s*)(.*)$/);
            if (assignment && replacements.length < MAX_SAFE_FIELDS) {
                const label = assignment[2]!.trim();
                const rawTail = assignment[4]!;
                const commentStart = splitInlineComment(rawTail);
                const valueWithWhitespace = rawTail.slice(0, commentStart);
                const leading = valueWithWhitespace.length - valueWithWhitespace.trimStart().length;
                const original = valueWithWhitespace.trim();
                const start = sourceOffset + assignment[1]!.length + assignment[2]!.length + assignment[3]!.length + leading;
                const id = `native-field-${order}`;
                const field = scalarField(id, label, section, original);
                replacements.push({ field, start, end: start + original.length, original, quote: decodeQuoted(original).quote });
                order += 1;
            } else {
                const command = line.match(/^(\s*)([A-Za-z0-9_.-]+)(\s+)(.+)$/);
                if (command && replacements.length < MAX_SAFE_FIELDS) {
                    const rawTail = command[4]!;
                    const commentStart = splitInlineComment(rawTail);
                    const valueWithWhitespace = rawTail.slice(0, commentStart);
                    const leading = valueWithWhitespace.length - valueWithWhitespace.trimStart().length;
                    const original = valueWithWhitespace.trim();
                    const label = command[2]!;
                    const start = sourceOffset + command[1]!.length + label.length + command[3]!.length + leading;
                    const id = `native-field-${order}`;
                    const field = scalarField(id, label, section, original);
                    replacements.push({ field, start, end: start + original.length, original, quote: decodeQuoted(original).quote });
                    order += 1;
                }
            }
        }
        sourceOffset += fullLine.length;
    }

    return {
        fields: sortFields(replacements.map(({ field }) => field)),
        serialize: (values) => replacements
            .map((replacement) => ({
                ...replacement,
                next: encodeScalar(
                    values[replacement.field.id] ?? replacement.field.value,
                    replacement.original,
                    replacement.quote,
                    replacement.field.kind === "secret",
                ),
            }))
            .sort((left, right) => right.start - left.start)
            .reduce((content, replacement) => `${content.slice(0, replacement.start)}${replacement.next}${content.slice(replacement.end)}`, source),
    };
}

function parseJsonModel(source: string): NativeConfigModel {
    const parsed = JSON.parse(source) as unknown;
    const entries: Array<{ field: NativeConfigField; path: string[] }> = [];
    let order = 0;

    const visit = (value: unknown, path: string[], depth: number) => {
        if (entries.length >= MAX_SAFE_FIELDS || depth > 4 || !value || typeof value !== "object" || Array.isArray(value)) return;
        for (const [key, nested] of Object.entries(value)) {
            if (["string", "number", "boolean"].includes(typeof nested)) {
                const id = `native-field-${order}`;
                const secret = isSecret(key);
                entries.push({
                    field: {
                        id,
                        label: key,
                        section: path.length > 0 ? path.join(" / ") : null,
                        kind: secret ? "secret" : typeof nested === "number" ? "number" : typeof nested === "boolean" ? "boolean" : "string",
                        value: secret ? "" : nested as NativeConfigScalar,
                        configured: String(nested).length > 0,
                    },
                    path: [...path, key],
                });
                order += 1;
            } else {
                visit(nested, [...path, key], depth + 1);
            }
        }
    };
    visit(parsed, [], 0);

    const indentation = source.match(/\n([ \t]+)["}]/)?.[1] ?? "  ";
    const trailingNewline = /\r?\n$/.test(source);
    return {
        fields: sortFields(entries.map(({ field }) => field)),
        serialize: (values) => {
            const next = JSON.parse(JSON.stringify(parsed)) as Record<string, unknown>;
            for (const entry of entries) {
                const submitted = values[entry.field.id] ?? entry.field.value;
                if (entry.field.kind === "secret" && submitted === "") continue;
                let target: Record<string, unknown> = next;
                for (const segment of entry.path.slice(0, -1)) target = target[segment] as Record<string, unknown>;
                target[entry.path.at(-1)!] = submitted;
            }
            return `${JSON.stringify(next, null, indentation)}${trailingNewline ? "\n" : ""}`;
        },
    };
}

function decodeXml(value: string): string {
    return value
        .replaceAll("&quot;", "\"")
        .replaceAll("&apos;", "'")
        .replaceAll("&lt;", "<")
        .replaceAll("&gt;", ">")
        .replaceAll("&amp;", "&");
}

function encodeXml(value: string): string {
    return value
        .replaceAll("&", "&amp;")
        .replaceAll("\"", "&quot;")
        .replaceAll("'", "&apos;")
        .replaceAll("<", "&lt;")
        .replaceAll(">", "&gt;");
}

function parseXmlModel(source: string): NativeConfigModel {
    const replacements: ReplacementField[] = [];
    let order = 0;
    for (const tagMatch of source.matchAll(/<property\b[^>]*>/gi)) {
        if (replacements.length >= MAX_SAFE_FIELDS) break;
        const tag = tagMatch[0];
        const name = tag.match(/\bname\s*=\s*(["'])(.*?)\1/i)?.[2];
        const value = tag.match(/\bvalue\s*=\s*(["'])(.*?)\1/i);
        if (!name || !value || value.index === undefined || tagMatch.index === undefined) continue;
        const quote = value[1] as "\"" | "'";
        const encoded = value[2]!;
        const valueOffset = value.index + value[0].indexOf(encoded);
        const id = `native-field-${order}`;
        const field = scalarField(id, decodeXml(name), null, `${quote}${decodeXml(encoded)}${quote}`);
        replacements.push({
            field,
            start: tagMatch.index + valueOffset,
            end: tagMatch.index + valueOffset + encoded.length,
            // Keep the exact encoded source so leaving a secret blank never
            // rewrites or exposes the configured value.
            original: encoded,
            quote: null,
        });
        order += 1;
    }
    return {
        fields: sortFields(replacements.map(({ field }) => field)),
        serialize: (values) => replacements
            .map((replacement) => {
                const submitted = values[replacement.field.id] ?? replacement.field.value;
                const next = replacement.field.kind === "secret" && submitted === ""
                    ? replacement.original
                    : encodeXml(String(submitted));
                return { ...replacement, next };
            })
            .sort((left, right) => right.start - left.start)
            .reduce((content, replacement) => `${content.slice(0, replacement.start)}${replacement.next}${content.slice(replacement.end)}`, source),
    };
}

export function parseNativeConfig(format: ConfigFileSummary["format"], source: string): NativeConfigModel {
    if (format === "json") return parseJsonModel(source);
    if (format === "xml") return parseXmlModel(source);
    if (["properties", "ini", "toml"].includes(format)) return parseLineModel(source);
    throw new Error(`Unsupported native configuration format: ${format}`);
}
