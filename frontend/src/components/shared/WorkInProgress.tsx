import { Construction } from "lucide-react";

interface WorkInProgressProps {
    title?: string;
    message?: string;
}

export default function WorkInProgress({
    title = "En Travaux",
    message = "Cette fonctionnalité sera bientôt disponible."
}: WorkInProgressProps) {
    return (
        <div className="wip-container">
            <Construction className="wip-icon" />
            <h3>{title}</h3>
            <p>{message}</p>
        </div>
    );
}
