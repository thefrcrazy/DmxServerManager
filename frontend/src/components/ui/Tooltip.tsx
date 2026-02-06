import { useState, useRef, ReactNode, useEffect, useCallback } from "react";
import { createPortal } from "react-dom";

interface TooltipProps {
    children: ReactNode;
    content: string;
    position?: "top" | "right" | "bottom" | "left";
    delay?: number;
    disabled?: boolean;
}

export default function Tooltip({
    children,
    content,
    position = "right",
    delay = 200,
    disabled = false
}: TooltipProps) {
    const [isVisible, setIsVisible] = useState(false);
    const [coords, setCoords] = useState({ top: 0, left: 0 });
    const triggerRef = useRef<HTMLDivElement>(null);
    const timeoutRef = useRef<NodeJS.Timeout | null>(null);

    const updatePosition = useCallback(() => {
        if (!triggerRef.current) return;
        const rect = triggerRef.current.getBoundingClientRect();

        // Calculate position based on prop
        let top = 0;
        let left = 0;
        const offset = 10; // spacing

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

        setCoords({ top, left });
    }, [position]);

    const handleMouseEnter = () => {
        if (disabled) return;
        updatePosition();
        timeoutRef.current = setTimeout(() => {
            setIsVisible(true);
        }, delay);
    };

    const handleMouseLeave = () => {
        if (timeoutRef.current) {
            clearTimeout(timeoutRef.current);
        }
        setIsVisible(false);
    };

    useEffect(() => {
        if (isVisible) {
            window.addEventListener("scroll", updatePosition);
            window.addEventListener("resize", updatePosition);
        }
        return () => {
            window.removeEventListener("scroll", updatePosition);
            window.removeEventListener("resize", updatePosition);
            if (timeoutRef.current) {
                clearTimeout(timeoutRef.current);
            }
        };
    }, [isVisible, updatePosition]);

    return (
        <>
            <div
                ref={triggerRef}
                className="tooltip-wrapper"
                onMouseEnter={handleMouseEnter}
                onMouseLeave={handleMouseLeave}
            >
                {children}
            </div>
            {isVisible && !disabled && createPortal(
                <div
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