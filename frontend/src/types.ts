export interface Server {
    id: string;
    name: string;
    game_type: string;
    status: string;
    executable_path: string;
    working_dir: string;
    auto_start: boolean;
    dir_exists: boolean;
    players?: string[];
    max_players?: number;
    cpu_usage: number;
    cpu_usage_normalized?: number; // Optional as api might not assume it yet
    memory_usage_bytes: number;
    max_memory_bytes: number;
    max_heap_bytes: number;
    disk_usage_bytes: number;
}
