import { useEffect, useMemo, useRef, useState } from "react";
import { EventEnvelope, EventEnvelopeSchema } from "@/schemas/api";
import { API_BASE_URL } from "@/services/api/base.client";

interface GlobalEventsOptions {
    enabled: boolean;
    eventTypes: readonly string[];
    onEvent: (event: EventEnvelope) => void;
    onResynchronize: () => void;
}

export function useGlobalEvents({ enabled, eventTypes, onEvent, onResynchronize }: GlobalEventsOptions) {
    const [isConnected, setIsConnected] = useState(false);
    const eventHandler = useRef(onEvent);
    const resynchronize = useRef(onResynchronize);
    eventHandler.current = onEvent;
    resynchronize.current = onResynchronize;
    const eventKey = useMemo(() => [...new Set(eventTypes)].sort().join("\u0000"), [eventTypes]);

    useEffect(() => {
        if (!enabled) {
            setIsConnected(false);
            return;
        }
        const source = new EventSource(`${API_BASE_URL}/events`, { withCredentials: true });
        const apply = (event: Event) => {
            if (!(event instanceof MessageEvent) || typeof event.data !== "string") return;
            let raw: unknown;
            try { raw = JSON.parse(event.data); } catch { return; }
            const parsed = EventEnvelopeSchema.safeParse(raw);
            if (parsed.success) eventHandler.current(parsed.data);
        };
        const reset = () => resynchronize.current();

        source.onopen = () => setIsConnected(true);
        source.onerror = () => {
            setIsConnected(false);
            // EventSource reconnects with Last-Event-ID, but a disconnected client
            // can still have missed events outside the server retention window.
            // Reload the bounded REST view immediately so persisted state remains
            // authoritative while the stream reconnects.
            resynchronize.current();
        };
        for (const type of eventKey.split("\u0000").filter(Boolean)) source.addEventListener(type, apply);
        source.addEventListener("stream.reset", reset);
        source.addEventListener("stream.lagged", reset);
        return () => {
            source.close();
            setIsConnected(false);
        };
    }, [enabled, eventKey]);

    return { isConnected };
}
