import { useState, useRef, ReactNode, useEffect, useCallback } from "react";
import { createPortal } from "react-dom";

interface TooltipProps {
    children: ReactNode;
    content: string;
    position?: "top" | "right" | "bottom" | "left";
    delay?: number;
    disabled?: boolean;
}

const EDGE_MARGIN = 8;

export default function Tooltip({
    children,
    content,
    position = "right",
    delay = 200,
    disabled = false
}: TooltipProps) {
    const [isVisible, setIsVisible] = useState(false);
    const [coords, setCoords] = useState({ top: 0, left: 0 });
    const wrapperRef = useRef<HTMLSpanElement>(null);
    const timeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

    const updatePosition = useCallback(() => {
        // Le conteneur est en `display: contents` pour ne pas perturber les
        // contextes flex : c'est donc l'élément déclencheur qu'il faut mesurer.
        const trigger = wrapperRef.current?.firstElementChild ?? wrapperRef.current;
        if (!trigger) return;
        const rect = trigger.getBoundingClientRect();

        let top = 0;
        let left = 0;
        const offset = 10;

        switch (position) {
            case "right":
                top = rect.top + rect.height / 2;
                left = rect.right + offset;
                break;
            case "left":
                top = rect.top + rect.height / 2;
                left = rect.left - offset;
                break;
            case "top":
                top = rect.top - (offset / 2);
                left = rect.left + rect.width / 2;
                break;
            case "bottom":
                top = rect.bottom + (offset / 2);
                left = rect.left + rect.width / 2;
                break;
        }

        setCoords({
            top: Math.min(Math.max(top, EDGE_MARGIN), window.innerHeight - EDGE_MARGIN),
            left: Math.min(Math.max(left, EDGE_MARGIN), window.innerWidth - EDGE_MARGIN),
        });
    }, [position]);

    const show = useCallback(() => {
        if (disabled) return;
        updatePosition();
        timeoutRef.current = setTimeout(() => setIsVisible(true), delay);
    }, [delay, disabled, updatePosition]);

    const hide = useCallback(() => {
        if (timeoutRef.current) clearTimeout(timeoutRef.current);
        setIsVisible(false);
    }, []);

    useEffect(() => {
        if (!isVisible) return;
        // En phase de capture : le défilement de cette application a lieu dans
        // `.page-container`, `.console-output` et `.table-scroll`, jamais sur
        // `window`. Sans capture, l'infobulle restait ancrée à des coordonnées
        // périmées et flottait au-dessus d'un contenu sans rapport.
        const options = { capture: true, passive: true } as const;
        const dismissOnEscape = (event: KeyboardEvent) => {
            if (event.key === "Escape") hide();
        };
        window.addEventListener("scroll", updatePosition, options);
        window.addEventListener("resize", updatePosition, options);
        document.addEventListener("keydown", dismissOnEscape);
        return () => {
            window.removeEventListener("scroll", updatePosition, options);
            window.removeEventListener("resize", updatePosition, options);
            document.removeEventListener("keydown", dismissOnEscape);
        };
    }, [hide, isVisible, updatePosition]);

    useEffect(() => () => {
        if (timeoutRef.current) clearTimeout(timeoutRef.current);
    }, []);

    return (
        <>
            <span
                ref={wrapperRef}
                className="tooltip-wrapper"
                onMouseEnter={show}
                onMouseLeave={hide}
                onFocus={show}
                onBlur={hide}
            >
                {children}
            </span>
            {isVisible && !disabled && createPortal(
                // `aria-hidden` est délibéré : chaque déclencheur porte déjà un
                // `aria-label` identique au contenu de l'infobulle. L'exposer une
                // seconde fois provoquerait une double annonce.
                <div
                    role="tooltip"
                    aria-hidden="true"
                    className={`tooltip tooltip--${position}`}
                    style={{
                        top: coords.top,
                        left: coords.left
                    }}
                >
                    {content}
                </div>,
                document.body
            )}
        </>
    );
}
