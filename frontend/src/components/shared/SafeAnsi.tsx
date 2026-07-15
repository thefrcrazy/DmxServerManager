import { CSSProperties, ReactNode } from "react";

const SGR_PATTERN = /(?:\u001b\[|\u009b)([\d;:]*)m/g;
const OSC_PATTERN = /(?:\u001b\]|\u009d)[\s\S]*?(?:\u0007|\u001b\\)/g;
const CONTROL_SEQUENCE_PATTERN = /[\u001b\u009b][[\]()#;?]*(?:(?:[a-zA-Z\d]*(?:;[-a-zA-Z\d/#&.:=?%@~_]+)*)?\u0007|(?:(?:\d{1,4}(?:[;:]\d{0,4})*)?[\dA-PR-TZcf-nq-uy=><~]))/g;

const COLORS = [
    "#161b22", "#f85149", "#3fb950", "#d29922",
    "#58a6ff", "#bc8cff", "#39c5cf", "#c9d1d9",
] as const;

export interface AnsiSegment {
    text: string;
    style: CSSProperties;
}

function indexedColor(index: number): string | undefined {
    if (index < 0 || index > 255) return undefined;
    if (index < 8) return COLORS[index];
    if (index < 16) return ["#6e7681", "#ff7b72", "#56d364", "#e3b341", "#79c0ff", "#d2a8ff", "#56d4dd", "#f0f6fc"][index - 8];
    if (index < 232) {
        const value = index - 16;
        const channel = (part: number) => part === 0 ? 0 : 55 + part * 40;
        return `rgb(${channel(Math.floor(value / 36))}, ${channel(Math.floor(value / 6) % 6)}, ${channel(value % 6)})`;
    }
    const gray = 8 + (index - 232) * 10;
    return `rgb(${gray}, ${gray}, ${gray})`;
}

function applySgr(style: CSSProperties, rawCodes: string): CSSProperties {
    const next = { ...style };
    const codes = (rawCodes || "0").replaceAll(":", ";").split(";").map((value) => Number(value || 0));
    for (let index = 0; index < codes.length; index += 1) {
        const code = codes[index];
        if (code === 0) Object.keys(next).forEach((key) => delete next[key as keyof CSSProperties]);
        else if (code === 1) next.fontWeight = 700;
        else if (code === 2) next.opacity = 0.72;
        else if (code === 3) next.fontStyle = "italic";
        else if (code === 4) next.textDecoration = "underline";
        else if (code === 22) { delete next.fontWeight; delete next.opacity; }
        else if (code === 23) delete next.fontStyle;
        else if (code === 24) delete next.textDecoration;
        else if (code >= 30 && code <= 37) next.color = COLORS[code - 30];
        else if (code >= 90 && code <= 97) next.color = indexedColor(code - 90 + 8);
        else if (code === 39) delete next.color;
        else if (code >= 40 && code <= 47) next.backgroundColor = COLORS[code - 40];
        else if (code >= 100 && code <= 107) next.backgroundColor = indexedColor(code - 100 + 8);
        else if (code === 49) delete next.backgroundColor;
        else if ((code === 38 || code === 48) && codes[index + 1] === 5) {
            const color = indexedColor(codes[index + 2]);
            if (color) next[code === 38 ? "color" : "backgroundColor"] = color;
            index += 2;
        } else if ((code === 38 || code === 48) && codes[index + 1] === 2) {
            const rgb = codes.slice(index + 2, index + 5);
            if (rgb.length === 3 && rgb.every((channel) => Number.isInteger(channel) && channel >= 0 && channel <= 255)) {
                next[code === 38 ? "color" : "backgroundColor"] = `rgb(${rgb.join(", ")})`;
            }
            index += 4;
        }
    }
    return next;
}

export function parseAnsi(input: string): AnsiSegment[] {
    const sanitized = input.replace(OSC_PATTERN, "");
    const segments: AnsiSegment[] = [];
    let style: CSSProperties = {};
    let cursor = 0;
    for (const match of sanitized.matchAll(SGR_PATTERN)) {
        const start = match.index ?? cursor;
        const text = sanitized.slice(cursor, start).replace(CONTROL_SEQUENCE_PATTERN, "");
        if (text) segments.push({ text, style: { ...style } });
        style = applySgr(style, match[1]);
        cursor = start + match[0].length;
    }
    const tail = sanitized.slice(cursor).replace(CONTROL_SEQUENCE_PATTERN, "");
    if (tail) segments.push({ text: tail, style: { ...style } });
    return segments;
}

interface SafeAnsiProps {
    children: ReactNode;
    useClasses?: boolean;
}

/** Renders a closed subset of ANSI SGR as React text nodes, never HTML or links. */
export default function SafeAnsi({ children }: SafeAnsiProps) {
    const text = typeof children === "string" || typeof children === "number" ? String(children) : "";
    return <>{parseAnsi(text).map((segment, index) => (
        <span key={index} style={segment.style}>{segment.text}</span>
    ))}</>;
}
